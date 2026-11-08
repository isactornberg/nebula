use super::*;

/// How often the transcript is read after a keypress mid-turn, and for how
/// long after the newest one: the cancel lands on disk a moment after the
/// key, later on a loaded machine.
const INTERRUPT_READ_EVERY: Duration = Duration::from_millis(300);
const INTERRUPT_WATCH_FOR: Duration = Duration::from_secs(4);

impl Daemon {
    // ---- status machine plumbing ----

    /// Feed one hook (or synthetic) event through the agent's status machine
    /// and apply the resulting effects (persist + broadcast).
    pub fn apply_hook_event(
        &self,
        agent_id: &AgentId,
        event: HookEvent,
        session_id: Option<String>,
    ) {
        enum Outcome {
            Effects(Vec<Effect>),
            UnknownAgent(HookEvent, Option<String>),
        }
        // A `nebula worktree` relocation waiting on this agent's turn end
        // holds that end at `running` (`AgentStatusMachine::set_relocating`;
        // `complete_pending_move` drains it). Read off the daemon's own
        // record before every event, so the machine's copy cannot go stale
        // — and before the machines lock, so the two are never nested.
        let relocating = self.pending_moves.lock().contains_key(agent_id);
        let session = self.session(&SessionRef::Agent(agent_id.clone()));
        if !event.synthetic() {
            if let Some(session) = &session {
                session.mark_heard();
            }
        }
        // The trust question seen opening by a watch the CLI's first hook
        // has since disarmed: that hook is past it, and no edge would ever
        // close it. Asked under the machines' lock, which the hook's own
        // event takes only after disarming, so one or the other is last.
        let trust_opened = matches!(event, HookEvent::TrustQuestion { open: true });
        let outcome = {
            let mut machines = self.status_machines.lock();
            if trust_opened && !session.as_ref().is_some_and(|s| s.trust_armed()) {
                return;
            }
            match machines.entry(agent_id.clone()) {
                std::collections::hash_map::Entry::Occupied(e) => {
                    let machine = e.into_mut();
                    machine.set_relocating(relocating);
                    Outcome::Effects(machine.handle(event, session_id.as_deref(), Instant::now()))
                }
                std::collections::hash_map::Entry::Vacant(slot) => {
                    // Lazily seed from the persisted row.
                    match self.store.get_agent(agent_id) {
                        Ok(Some(agent)) => {
                            let machine = slot
                                .insert(AgentStatusMachine::new(agent.status, agent.session_id));
                            machine.set_relocating(relocating);
                            Outcome::Effects(machine.handle(
                                event,
                                session_id.as_deref(),
                                Instant::now(),
                            ))
                        }
                        _ => Outcome::UnknownAgent(event, session_id),
                    }
                }
            }
        };
        match outcome {
            Outcome::Effects(effects) => self.apply_status_effects(agent_id, effects),
            // Ids with no row are prewarmed sessions (buffer for replay at
            // adoption) or stale env / deleted agents (dropped, as before).
            Outcome::UnknownAgent(event, session_id) => {
                self.buffer_prewarm_hook(agent_id, event, session_id)
            }
        }
    }

    pub(super) fn buffer_prewarm_hook(
        &self,
        agent_id: &AgentId,
        event: HookEvent,
        session_id: Option<String>,
    ) {
        let mut pool = self.prewarmed.lock();
        if let Some(entry) = pool.values_mut().find(|e| &e.agent_id == agent_id) {
            if entry.buffered_hooks.len() >= PREWARM_HOOK_BUFFER_CAP {
                entry.buffered_hooks.remove(0);
            }
            entry.buffered_hooks.push((event, session_id));
        }
    }

    /// A keypress reached `id`'s PTY. Escaping a turn or a permission
    /// dialog fires no hook, and the progress bar that reports it is drawn
    /// in few terminals (`HookEvent::Interrupted`), so while a turn is live
    /// the transcript is read over the seconds after the newest key, since
    /// Claude writes the cancel a moment after it.
    pub fn watch_for_interrupt(self: &Arc<Self>, id: &AgentId) {
        let cancellable = self
            .status_machines
            .lock()
            .get(id)
            .is_some_and(AgentStatusMachine::cancellable);
        if !cancellable || !self.transcripts.lock().contains_key(id) {
            return;
        }
        // A key during a live watch only moves its end, so Esc seconds after
        // a Down that started it is still read.
        if self
            .interrupt_watches
            .lock()
            .insert(id.clone(), Instant::now())
            .is_some()
        {
            return;
        }
        let daemon = self.clone();
        let id = id.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(INTERRUPT_READ_EVERY).await;
                let transcript = daemon.transcripts.lock().get(&id).cloned();
                if transcript.is_some_and(|t| t.interrupted()) {
                    daemon.interrupt_watches.lock().remove(&id);
                    daemon.apply_hook_event(&id, HookEvent::Interrupted, None);
                    daemon.complete_pending_move(&id, &HookEvent::Interrupted);
                    return;
                }
                let mut watches = daemon.interrupt_watches.lock();
                if watches
                    .get(&id)
                    .is_none_or(|newest| newest.elapsed() >= INTERRUPT_WATCH_FOR)
                {
                    watches.remove(&id);
                    return;
                }
            }
        });
    }

    /// Deferred-finish recheck across all machines (runs on a timer).
    pub fn tick_status_machines(&self) {
        let now = Instant::now();
        let ticked: Vec<(AgentId, Vec<Effect>)> = {
            let mut machines = self.status_machines.lock();
            machines
                .iter_mut()
                .map(|(id, m)| (id.clone(), m.tick(now)))
                .collect()
        };
        for (id, effects) in ticked {
            self.apply_status_effects(&id, effects);
        }
    }

    pub(super) fn apply_status_effects(&self, agent_id: &AgentId, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::SetStatus(status) => {
                    let (changed_at, unseen) = match self.store.set_agent_status(agent_id, status) {
                        Ok(stamped) => stamped,
                        Err(e) => {
                            tracing::warn!(error = %e, "persist status failed");
                            (nebula_core::clock::now_ms(), false)
                        }
                    };
                    self.broadcast(ServerEvent::StatusChanged {
                        agent: agent_id.clone(),
                        status,
                        changed_at,
                        unseen,
                    });
                }
                Effect::SaveSessionId(sid) => {
                    if let Err(e) = self.store.set_agent_session_id(agent_id, Some(&sid)) {
                        tracing::warn!(error = %e, "persist session id failed");
                    }
                }
            }
        }
    }
}
