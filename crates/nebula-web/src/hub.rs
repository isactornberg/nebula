//! The one connection `nebula serve` holds to the DAEMON, and what it
//! keeps from it: the entity tree, the BOARD READS taken on the board's
//! beat, the answers the focus picks' replies offer, and the STATE built
//! from them, published at most once every [`PUBLISH_EVERY`]. A handler
//! reads the STATE off the hub and asks the daemon for anything else (a
//! reply, a screen, keys typed) through it, since one task owns the
//! connection and reads every event that comes back.
//!
//! The hub never starts a daemon. With none up, or once the one it had
//! drops the connection (an IN-PLACE RESTART drops every client), there is
//! no STATE, and it connects again, waiting longer between tries up to
//! [`RETRY_MAX`].

use crate::{answers, api};
use nebula_client::board::{self, BoardRead};
use nebula_client::screen::SCREEN_TAIL_BYTES;
use nebula_client::Client;
use nebula_core::{AgentId, ClientRequest, OutputTail, ServerEvent, SessionRef};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;

/// The STATE is published at most this often, however fast events come.
const PUBLISH_EVERY: Duration = Duration::from_millis(100);
/// How long to wait before connecting again, at first and at most.
const RETRY_MIN: Duration = Duration::from_millis(250);
const RETRY_MAX: Duration = Duration::from_secs(5);
/// How long a handler waits for the daemon's answer.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);
/// How old a focus pick's state must be before its reply is read: Claude
/// can write the reply to its transcript a moment after the turn ends, and
/// the daemon gives it this long when it waits on one.
const REPLY_SETTLE: Duration = Duration::from_secs(3);

/// The STATE as last published, the JSON it was sent as, the protocol of
/// the daemon it came from, and each project's checkouts.
pub struct Live {
    pub state: api::State,
    pub json: String,
    pub protocol: u32,
    pub repos: Vec<crate::skills::ProjectRoot>,
}

/// Why the daemon did not answer.
#[derive(Debug)]
pub enum Unanswered {
    /// No daemon is connected, or it went away before answering.
    Down,
    /// It took longer than [`ANSWER_TIMEOUT`].
    Late,
    /// It refused, saying why.
    Refused(String),
}

type Answer<T> = oneshot::Sender<Result<T, String>>;

enum Ask {
    Reply(AgentId, Answer<Option<String>>),
    Screen(AgentId, Answer<Option<OutputTail>>),
    Input(AgentId, Vec<u8>, Answer<()>),
    Seen(AgentId, Answer<()>),
}

/// A handle on the hub's task, cheap to clone into every handler.
#[derive(Clone)]
pub struct Hub {
    live: watch::Receiver<Option<Arc<Live>>>,
    asks: mpsc::Sender<Ask>,
}

impl Hub {
    /// Start the task that connects to the daemon and keeps the STATE.
    pub fn spawn() -> Hub {
        let (live_tx, live) = watch::channel(None);
        let (asks, asks_rx) = mpsc::channel(64);
        tokio::spawn(run(live_tx, asks_rx));
        Hub { live, asks }
    }

    /// A hub with no daemon, ever: what the server's own tests stand in.
    #[cfg(test)]
    pub fn down() -> Hub {
        let (_, live) = watch::channel(None);
        let (asks, _) = mpsc::channel(1);
        Hub { live, asks }
    }

    /// The STATE now, `None` while no daemon is connected.
    pub fn live(&self) -> Option<Arc<Live>> {
        self.live.borrow().clone()
    }

    /// Every STATE from now on, as it is published.
    pub fn subscribe(&self) -> watch::Receiver<Option<Arc<Live>>> {
        self.live.clone()
    }

    /// The last reply of Claude session `id`, every line of it, as its
    /// transcript holds it (`ReadSession`'s report); `None` for a session
    /// with no report to read, and from a daemon too old to be asked.
    pub async fn reply(&self, id: AgentId) -> Result<Option<String>, Unanswered> {
        self.ask(|tx| Ask::Reply(id, tx)).await
    }

    /// The end of session `id`'s ring (`TailOutput`); `None` when it has no
    /// live PTY.
    pub async fn screen(&self, id: AgentId) -> Result<Option<OutputTail>, Unanswered> {
        self.ask(|tx| Ask::Screen(id, tx)).await
    }

    /// Type `data` into session `id`'s PTY as keys, as a TUI showing it
    /// would, without attaching: an Attach resizes the PTY.
    pub async fn input(&self, id: AgentId, data: Vec<u8>) -> Result<(), Unanswered> {
        self.ask(|tx| Ask::Input(id, data, tx)).await
    }

    /// Session `id`'s finish has been looked at (`MarkAgentSeen`).
    pub async fn seen(&self, id: AgentId) -> Result<(), Unanswered> {
        self.ask(|tx| Ask::Seen(id, tx)).await
    }

    async fn ask<T>(&self, ask: impl FnOnce(Answer<T>) -> Ask) -> Result<T, Unanswered> {
        let (tx, rx) = oneshot::channel();
        self.asks
            .send(ask(tx))
            .await
            .map_err(|_| Unanswered::Down)?;
        match tokio::time::timeout(ANSWER_TIMEOUT, rx).await {
            Err(_) => Err(Unanswered::Late),
            Ok(Err(_)) => Err(Unanswered::Down),
            Ok(Ok(answer)) => answer.map_err(Unanswered::Refused),
        }
    }
}

/// Connect, keep the STATE while connected, and connect again once the
/// daemon is gone, for as long as the server runs. What it could not
/// connect for is said once, until it changes.
async fn run(live: watch::Sender<Option<Arc<Live>>>, mut asks: mpsc::Receiver<Ask>) {
    let mut retry = RETRY_MIN;
    let mut said = String::new();
    loop {
        match Client::connect("nebula serve waits for one to start").await {
            Ok(client) => {
                eprintln!("nebula serve: connected to the daemon");
                said.clear();
                retry = RETRY_MIN;
                Connected::new(client).run(&live, &mut asks).await;
                live.send_replace(None);
                eprintln!("nebula serve: the daemon went away; connecting again");
            }
            Err(err) => {
                let why = format!("{err:#}");
                if why != said {
                    eprintln!("nebula serve: {why}");
                    said = why;
                }
            }
        }
        // Nothing can be asked meanwhile: an ask is dropped, which its
        // handler takes for a daemon that is down.
        let wait = tokio::time::sleep(retry);
        tokio::pin!(wait);
        loop {
            tokio::select! {
                _ = &mut wait => break,
                ask = asks.recv() => if ask.is_none() { return },
            }
        }
        retry = (retry * 2).min(RETRY_MAX);
    }
}

/// What a request the hub sent is waiting for.
enum Pending {
    BoardRead(AgentId),
    /// A focus pick's reply, read in that state of its session.
    Answers(AgentId, (api::SessionStatus, i64)),
    Reply(Answer<Option<String>>),
    Screen(Answer<Option<OutputTail>>),
}

/// One connection, from its Snapshot until the daemon hangs up.
struct Connected {
    client: Client,
    reads: HashMap<AgentId, BoardRead>,
    answers: HashMap<AgentId, api::PickAnswers>,
    pending: HashMap<u64, Pending>,
    /// The STATE last published.
    published: Option<Arc<Live>>,
}

impl Connected {
    fn new(client: Client) -> Self {
        Connected {
            client,
            reads: HashMap::new(),
            answers: HashMap::new(),
            pending: HashMap::new(),
            published: None,
        }
    }

    async fn run(
        mut self,
        live: &watch::Sender<Option<Arc<Live>>>,
        asks: &mut mpsc::Receiver<Ask>,
    ) {
        let mut beat = tokio::time::interval(board::READ_BEAT);
        beat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut changed = true;
        let mut published = Instant::now() - PUBLISH_EVERY;
        loop {
            let publish_at = published + PUBLISH_EVERY;
            let sent = tokio::select! {
                event = self.client.next_event() => match event {
                    Ok(event) => {
                        changed |= self.land(event);
                        Ok(())
                    }
                    Err(_) => return,
                },
                Some(ask) = asks.recv() => self.ask(ask).await,
                _ = beat.tick() => match self.read_board().await {
                    Ok(()) => self.read_answers().await,
                    err => err,
                },
                _ = tokio::time::sleep_until(publish_at), if changed => {
                    let now = Arc::new(self.live());
                    live.send_replace(Some(now.clone()));
                    self.published = Some(now);
                    published = Instant::now();
                    changed = false;
                    Ok(())
                }
            };
            if sent.is_err() {
                return;
            }
        }
    }

    /// The STATE of the tree as it stands, and its JSON.
    fn live(&self) -> Live {
        let state = api::state(
            &self.client.tree,
            &self.reads,
            &self.answers,
            &api::spaces(),
        );
        let json = serde_json::to_string(&state).expect("the STATE serializes");
        let tree = &self.client.tree;
        Live {
            state,
            json,
            protocol: self.client.protocol,
            repos: tree
                .projects
                .iter()
                .filter(|p| !tree.is_orchestrator_home(p))
                .map(|p| crate::skills::ProjectRoot {
                    name: p.name.clone(),
                    root: p.repo_path.clone(),
                    worktrees: tree
                        .worktrees
                        .iter()
                        .chain(&tree.removed_worktrees)
                        .filter(|w| w.project_id == p.id && w.path != p.repo_path)
                        .map(|w| w.path.clone())
                        .collect(),
                    space: None,
                })
                .collect(),
        }
    }

    /// Fold one event in. Whether the STATE changed with it.
    fn land(&mut self, event: ServerEvent) -> bool {
        match event {
            ServerEvent::Snapshot { .. }
            | ServerEvent::EntityUpserted { .. }
            | ServerEvent::EntityRemoved { .. }
            | ServerEvent::StatusChanged { .. }
            | ServerEvent::TuiShowing { .. } => true,
            ServerEvent::OutputTail { req_id, tail, .. } => match self.pending.remove(&req_id) {
                Some(Pending::BoardRead(id)) => {
                    board::land_read(&mut self.reads, &id, tail.as_ref())
                }
                Some(Pending::Screen(tx)) => {
                    let _ = tx.send(Ok(tail));
                    false
                }
                _ => false,
            },
            ServerEvent::SessionText { req_id, text, .. } => {
                let text = text.strip_suffix('\n').unwrap_or(&text);
                match self.pending.remove(&req_id) {
                    Some(Pending::Reply(tx)) => drop(tx.send(Ok(Some(text.to_string())))),
                    Some(Pending::Answers(id, state)) => {
                        return self.land_answers(&id, state, answers::answers_of(text));
                    }
                    _ => {}
                }
                false
            }
            ServerEvent::Error {
                req_id: Some(req_id),
                message,
            } => {
                match self.pending.remove(&req_id) {
                    // A session with no transcript, or no reply in it yet.
                    Some(Pending::Reply(tx)) => drop(tx.send(Ok(None))),
                    Some(Pending::Answers(id, state)) => {
                        return self.land_answers(&id, state, Vec::new());
                    }
                    Some(Pending::Screen(tx)) => drop(tx.send(Err(message))),
                    Some(Pending::BoardRead(_)) | None => {}
                }
                false
            }
            _ => false,
        }
    }

    /// The answers a focus pick's reply offers, read in `state` of its
    /// session; whether the STATE changed with them. One read in a state
    /// the session has since left is dropped.
    fn land_answers(
        &mut self,
        id: &AgentId,
        state: (api::SessionStatus, i64),
        got: Vec<String>,
    ) -> bool {
        match self.answers.get_mut(id) {
            Some(pick) if pick.state == state => {
                pick.answers = Some(got);
                true
            }
            _ => false,
        }
    }

    /// Ask for the reply of each focus pick not read in the state its
    /// session is in, once that is [`REPLY_SETTLE`] old, by the rule the
    /// detail reads answers by: only a Claude session's reply offers any.
    async fn read_answers(&mut self) -> anyhow::Result<()> {
        let Some(live) = self.published.clone() else {
            return Ok(());
        };
        let state = &live.state;
        let settled_before = nebula_core::clock::now_ms() - REPLY_SETTLE.as_millis() as i64;
        let picks: Vec<&api::Session> = state
            .focus
            .picks
            .iter()
            .filter_map(|p| state.sessions.iter().find(|s| s.id == p.session_id))
            .collect();
        self.answers
            .retain(|id, _| picks.iter().any(|s| s.id == id.0));
        if self.client.protocol < nebula_core::READ_REPORT_PROTOCOL {
            return Ok(());
        }
        for s in picks {
            let id = AgentId(s.id.clone());
            let at = (s.status, s.status_changed_at);
            let read = self.answers.get(&id).is_some_and(|a| a.state == at);
            if read || s.harness != api::Harness::Claude || s.status_changed_at > settled_before {
                continue;
            }
            let req_id = self.client.next_req_id();
            self.client.send(&report_of(req_id, &id)).await?;
            self.pending
                .insert(req_id, Pending::Answers(id.clone(), at));
            self.answers.insert(
                id,
                api::PickAnswers {
                    state: at,
                    answers: None,
                },
            );
        }
        Ok(())
    }

    /// Send what a handler asks for, the answer to come back as an event.
    async fn ask(&mut self, ask: Ask) -> anyhow::Result<()> {
        let req_id = self.client.next_req_id();
        let (request, pending) = match ask {
            Ask::Reply(_, tx) if self.client.protocol < nebula_core::READ_REPORT_PROTOCOL => {
                let _ = tx.send(Ok(None));
                return Ok(());
            }
            Ask::Reply(id, tx) => (report_of(req_id, &id), Pending::Reply(tx)),
            Ask::Screen(id, tx) => (tail_of(req_id, id), Pending::Screen(tx)),
            Ask::Input(id, data, tx) => {
                let input = ClientRequest::Input {
                    session: SessionRef::Agent(id),
                    data,
                };
                self.client.send(&input).await?;
                let _ = tx.send(Ok(()));
                return Ok(());
            }
            Ask::Seen(id, tx) => {
                self.client
                    .send(&ClientRequest::MarkAgentSeen { id })
                    .await?;
                let _ = tx.send(Ok(()));
                return Ok(());
            }
        };
        self.client.send(&request).await?;
        self.pending.insert(req_id, pending);
        Ok(())
    }

    /// Ask for the screen of every session due a BOARD READ.
    async fn read_board(&mut self) -> anyhow::Result<()> {
        let now = nebula_core::clock::now_ms();
        for id in api::due_reads(&self.client.tree, &mut self.reads, now) {
            let req_id = self.client.next_req_id();
            self.client.send(&tail_of(req_id, id.clone())).await?;
            self.pending.insert(req_id, Pending::BoardRead(id));
        }
        Ok(())
    }
}

/// The ask for Claude session `id`'s last reply, every line of it.
fn report_of(req_id: u64, id: &AgentId) -> ClientRequest {
    ClientRequest::ReadSession {
        req_id,
        target: id.0.clone(),
        max_lines: 0,
        report: true,
    }
}

/// The ask for the end of session `id`'s ring, enough of it for a screen.
fn tail_of(req_id: u64, id: AgentId) -> ClientRequest {
    ClientRequest::TailOutput {
        req_id,
        session: SessionRef::Agent(id),
        max_bytes: SCREEN_TAIL_BYTES,
        after_seq: None,
    }
}
