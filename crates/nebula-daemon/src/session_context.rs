//! Agent-readable session context commands: list, read, ask, and the wait
//! for a session to be done.
//!
//! These are the non-privileged counterparts to the orchestrator's
//! list/read/send machinery. They use the same daemon tree, PTY delivery and
//! status broadcasts, but do not require the caller to be the orchestrator.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use nebula_core::{
    Agent, AgentId, AgentKind, AgentStatus, OutputTail, Project, ServerEvent, SessionRef,
    SessionSummary, TerminalTab, Worktree, DONE_SETTLE, MAX_CLOUD_PROMPT_BYTES, TURN_RECEIPT_WAIT,
};
use tokio::sync::broadcast::error::RecvError;

use crate::registry::Daemon;

const READ_MAX_BYTES: usize = 256 * 1024;
const ASK_OUTPUT_MAX_BYTES: usize = 64 * 1024;
/// How long an ask gives Claude's reply to land in its transcript once
/// the turn has ended, and how often it looks meanwhile.
const REPLY_WAIT: Duration = Duration::from_secs(3);
const REPLY_POLL: Duration = Duration::from_millis(100);
/// How much of a transcript an ask reads for the reply at most: a turn's
/// tool results can be large.
const REPLY_SCAN_BYTES: u64 = 8 * 1024 * 1024;
const DEFAULT_ASK_TIMEOUT: Duration = Duration::from_secs(120);
/// How often [`Daemon::await_done`] reads the row, and the process tree
/// under it once the row reads done.
const DONE_POLL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
pub struct SessionRead {
    pub session: SessionRef,
    pub text: String,
}

#[derive(Debug, Clone)]
struct SessionLookup {
    summary: SessionSummary,
}

impl Daemon {
    /// Model-facing rows for every non-archived session across every project.
    pub fn session_summaries(&self) -> Result<Vec<SessionSummary>> {
        let (projects, worktrees, mut agents, mut terminals) = self.store.load_tree()?;
        let projects = projects
            .into_iter()
            .map(|p| (p.id.clone(), p))
            .collect::<HashMap<_, _>>();
        let worktrees = worktrees
            .into_iter()
            .map(|w| (w.id.clone(), w))
            .collect::<HashMap<_, _>>();

        for agent in &mut agents {
            agent.alive = self.is_alive(&SessionRef::Agent(agent.id.clone()));
        }
        for terminal in &mut terminals {
            terminal.alive = self.is_alive(&SessionRef::Terminal(terminal.id.clone()));
        }

        let mut rows = Vec::new();
        for agent in agents.into_iter().filter(|a| !a.archived) {
            if let Some(row) = agent_summary(&projects, &worktrees, agent) {
                rows.push(row);
            }
        }
        for terminal in terminals {
            if let Some(row) = terminal_summary(&projects, &worktrees, terminal) {
                rows.push(row);
            }
        }
        rows.sort_by(|a, b| {
            a.project
                .cmp(&b.project)
                .then_with(|| a.branch.cmp(&b.branch))
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(rows)
    }

    /// Resolve a user-facing session selector by id/name with unambiguous
    /// fuzzy fallback.
    pub fn resolve_session_target(&self, raw: &str) -> Result<SessionRef> {
        let target = raw.trim();
        if target.is_empty() {
            bail!("session name or id is empty");
        }
        let rows = self
            .session_summaries()?
            .into_iter()
            .map(|summary| SessionLookup { summary })
            .collect::<Vec<_>>();
        resolve_in_rows(&rows, target).map(|row| row.summary.session.clone())
    }

    /// Clean recent context for `target`, preferring a known Claude
    /// transcript and falling back to the live PTY scrollback. An archived
    /// session, or one whose row went with its checkout (a merged
    /// worktree's cleanup), is read by its exact id: a CAPTAIN reads the
    /// final replies of a merged branch's workers after the fact.
    pub fn read_session_context(&self, target: &str, max_lines: u32) -> Result<SessionRead> {
        let session = match self.resolve_session_target(target) {
            Ok(session) => session,
            Err(err) => self.retired_agent(target).ok_or(err)?,
        };
        let text = self.read_session_ref(&session, max_lines)?;
        Ok(SessionRead { session, text })
    }

    /// Read a known session ref: its Claude transcript, else the screen
    /// its live PTY shows ([`screen_text`]).
    pub fn read_session_ref(&self, session: &SessionRef, max_lines: u32) -> Result<String> {
        if let SessionRef::Agent(id) = session {
            if let Some(text) = self.read_agent_transcript(id, max_lines) {
                return Ok(text);
            }
        }
        let text = self
            .session(session)
            .map(|s| screen_text(&s))
            .unwrap_or_else(|| "(session is not live and no transcript is available)\n".into());
        Ok(limit_lines(&text, max_lines))
    }

    /// `nebula orchestrator read`: a Claude session's recent conversation
    /// from its transcript, as `nebula read` prints it, with the screen
    /// under it while a dialog is open there, which no transcript holds;
    /// any other session's screen ([`screen_text`]). The last `max_bytes`
    /// of that, in a tail at the PTY's size; `None` with neither a live PTY
    /// nor a transcript.
    pub fn read_output_tail_clean(
        &self,
        session: &SessionRef,
        max_bytes: u32,
    ) -> Option<OutputTail> {
        let max = (max_bytes as usize).min(READ_MAX_BYTES);
        let pty = self.session(session);
        let (transcript, dialog) = match session {
            SessionRef::Agent(id) => (
                self.read_agent_transcript(id, u32::MAX),
                self.store
                    .get_agent(id)
                    .ok()
                    .flatten()
                    .is_some_and(|a| a.status == AgentStatus::NeedsFeedback),
            ),
            SessionRef::Terminal(_) => (None, false),
        };
        let text = match (transcript, &pty) {
            (Some(text), Some(pty)) if dialog => {
                format!(
                    "{text}\n(its screen, with a dialog open:)\n{}",
                    screen_text(pty)
                )
            }
            (Some(text), _) => text,
            (None, Some(pty)) => screen_text(pty),
            (None, None) => return None,
        };
        let mut tail = pty.map_or(
            OutputTail {
                cols: 0,
                rows: 0,
                end_seq: 0,
                data: Vec::new(),
            },
            |pty| pty.tail(0, None),
        );
        tail.data = last_bytes(&text, max).as_bytes().to_vec();
        Some(tail)
    }

    /// The archived or deleted session `raw` is the exact id of. A deleted
    /// row is known only by the transcript a hook named since the daemon
    /// started.
    fn retired_agent(&self, raw: &str) -> Option<SessionRef> {
        let id = AgentId(raw.trim().to_string());
        let known = self.transcripts.lock().contains_key(&id)
            || self.store.get_agent(&id).ok().flatten().is_some();
        known.then_some(SessionRef::Agent(id))
    }

    fn read_agent_transcript(&self, id: &AgentId, max_lines: u32) -> Option<String> {
        transcript_context(&self.agent_transcript_path(id)?, max_lines)
    }

    /// `id`'s Claude transcript: where its hooks said, or, for a row no hook
    /// has named it for since the daemon started, `<session id>.jsonl` in
    /// one of Claude's projects dirs.
    fn agent_transcript_path(&self, id: &AgentId) -> Option<std::path::PathBuf> {
        let known = self
            .transcripts
            .lock()
            .get(id)
            .map(|t| t.transcript_path.clone());
        if known.is_some() {
            return known;
        }
        let agent = self.store.get_agent(id).ok().flatten()?;
        if agent.kind != AgentKind::Claude {
            return None;
        }
        crate::registry::claude_transcript_file(
            &self.claude_projects_dirs(),
            agent.session_id.as_deref()?,
        )
    }

    /// The reply a Claude session `id` gave to the turn typed in once its
    /// transcript stood at `mark` (its path and length then): the newest
    /// assistant message written past that ([`newest_reply`]), looked for
    /// over [`REPLY_WAIT`], within the ask's `deadline`, since Claude can
    /// write it a moment after the turn's end. A transcript other than the
    /// marked one, as a resume can start, is read whole. `None` at once
    /// when the session has no transcript at all.
    async fn reply_after(
        &self,
        id: &AgentId,
        mark: Option<(std::path::PathBuf, u64)>,
        deadline: tokio::time::Instant,
    ) -> Option<String> {
        let give_up = deadline.min(tokio::time::Instant::now() + REPLY_WAIT);
        loop {
            let path = self.agent_transcript_path(id)?;
            let from = match &mark {
                Some((marked, len)) if *marked == path => *len,
                _ => 0,
            };
            if let Some(reply) = newest_reply(&path, from) {
                return Some(reply);
            }
            if tokio::time::Instant::now() >= give_up {
                return None;
            }
            tokio::time::sleep(REPLY_POLL).await;
        }
    }

    /// Deliver `question` into another agent and return its answer: for a
    /// Claude session, the reply its transcript holds for that turn, and
    /// otherwise, or with none there, the screen the turn left
    /// ([`screen_text`]).
    pub async fn ask_session(
        self: &Arc<Self>,
        caller: Option<AgentId>,
        target: &str,
        question: &str,
        timeout: Duration,
        wait: bool,
    ) -> Result<(AgentId, String)> {
        let timeout = if timeout.is_zero() {
            DEFAULT_ASK_TIMEOUT
        } else {
            timeout
        };
        let deadline = tokio::time::Instant::now() + timeout;
        let target_ref = self.resolve_session_target(target)?;
        let SessionRef::Agent(target_id) = target_ref else {
            bail!("questions can only be sent to agent sessions");
        };
        if caller.as_ref() == Some(&target_id) {
            bail!("cannot ask the current session; choose another session");
        }
        let target_agent = self
            .store
            .get_agent(&target_id)?
            .context("target agent not found")?;
        if target_agent.archived {
            bail!("target agent is archived");
        }
        if target_agent.cloud_session_id.is_some() {
            bail!("target agent is a Claude Cloud session; use its cloud message path");
        }

        let mut status_rx = self.events.subscribe();
        let current = target_agent.status;
        if !is_idle(current) {
            if !wait {
                bail!(
                    "target session is {}; retry without --no-wait or choose an idle session",
                    current.as_str()
                );
            }
            wait_until_idle(&mut status_rx, &target_id, deadline).await?;
        }

        let caller_name = caller
            .as_ref()
            .and_then(|id| self.store.get_agent(id).ok().flatten())
            .map(|a| a.name)
            .unwrap_or_else(|| "external nebula CLI".into());
        let prompt = labelled_question(&caller_name, question)?;
        let session_ref = SessionRef::Agent(target_id.clone());
        let session = self.ensure_session(
            &session_ref,
            crate::pty::DEFAULT_COLS,
            crate::pty::DEFAULT_ROWS,
        )?;
        // Never into a question the CLI asks as it starts, or a dialog it
        // shows past that: the text and its Enter would answer it.
        if let Err(why) = self.await_session_up(&target_id, Duration::ZERO).await {
            let dir = self
                .store
                .get_worktree(&target_agent.worktree_id)?
                .map(|w| w.path)
                .unwrap_or_default();
            bail!(
                "nothing was asked: {}",
                why.say(&target_id, &dir, false, "ask again")
            );
        }
        let claude = target_agent.kind == AgentKind::Claude;
        let mark = self
            .agent_transcript_path(&target_id)
            .filter(|_| claude)
            .map(|path| {
                let len = std::fs::metadata(&path).map_or(0, |m| m.len());
                (path, len)
            });
        self.type_turn(&target_id, &session, &prompt).await?;

        wait_for_answer_turn(&mut status_rx, &target_id, deadline).await?;
        let reply = if claude {
            self.reply_after(&target_id, mark, deadline).await
        } else {
            None
        };
        let answer = reply.unwrap_or_else(|| screen_text(&session));
        Ok((
            target_id,
            last_bytes(&answer, ASK_OUTPUT_MAX_BYTES).to_string(),
        ))
    }

    /// Wait until the agent `id` is done (`ClientRequest::AwaitDone`): its
    /// status is one a turn ends in ([`is_idle`]), and has been for
    /// [`DONE_SETTLE`] by the stamp the change was persisted with, so a
    /// Stop that a subagent's start heals back to running is not taken for
    /// the end. A job the agent cut loose from its terminal (a
    /// `run_in_background` Bash call, a Monitor watch) counts as work too:
    /// the agent wakes to read it when it ends, so the settle runs from the
    /// last moment one was seen. A row archived or deleted meanwhile is the
    /// error: there is no turn left to wait for.
    pub async fn await_done(self: &Arc<Self>, id: &AgentId) -> Result<AgentStatus> {
        let mut job_seen_at = 0;
        loop {
            let daemon = self.clone();
            let agent_id = id.clone();
            let (agent, job) = tokio::task::spawn_blocking(move || -> Result<(Agent, bool)> {
                let Some(agent) = daemon.store.get_agent(&agent_id)? else {
                    bail!("session {agent_id} was deleted: there is no turn to wait for");
                };
                if agent.archived {
                    bail!("session {agent_id} is archived: there is no turn to wait for");
                }
                let job = is_idle(agent.status)
                    && daemon
                        .session(&SessionRef::Agent(agent_id))
                        .and_then(|session| session.child_pid)
                        .is_some_and(crate::pty::detached_job_under);
                Ok((agent, job))
            })
            .await??;
            let now = nebula_core::clock::now_ms();
            if job {
                job_seen_at = now;
            }
            let since = agent.status_changed_at.max(job_seen_at);
            if is_idle(agent.status) && now - since >= DONE_SETTLE.as_millis() as i64 {
                return Ok(agent.status);
            }
            tokio::time::sleep(DONE_POLL).await;
        }
    }
}

impl Daemon {
    /// `ClientRequest::TypeTurn`: `nebula session send`'s typing, into the
    /// live terminal of `id`, which the CLI found at rest and taking input
    /// ([`Self::type_turn`]).
    pub async fn session_send(&self, id: &AgentId, text: &str) -> Result<bool> {
        let text = crate::orchestrator::validate_message(text)?;
        let session = self
            .session(&SessionRef::Agent(id.clone()))
            .context("nothing was typed: the session has no live terminal")?;
        self.type_turn(id, &session, text).await
    }

    /// A prompt `id`'s CLI reported submitting, whole, to every typed turn
    /// waiting to hear one ([`Self::type_turn`]).
    pub(crate) fn prompt_submitted(&self, id: &AgentId, text: String) {
        if self.submitted_prompts.receiver_count() > 0 {
            let _ = self.submitted_prompts.send((id.clone(), text.into()));
        }
    }

    /// Type `text` into `session`, agent `id`'s, as its next turn
    /// ([`write_prompt`]), then hear what its CLI took: `true` once it
    /// reports submitting a prompt that holds the whole of `text`
    /// ([`crate::prompt_history::holds`]), `false` when it reports none
    /// within [`TURN_RECEIPT_WAIT`]. A prompt that holds only part of it is
    /// the error, since the session is working on that part. One the CLI
    /// injected into its own turn queue meanwhile is no answer
    /// ([`crate::prompt_history::is_injected`]).
    pub(crate) async fn type_turn(
        &self,
        id: &AgentId,
        session: &Arc<crate::pty::PtySession>,
        text: &str,
    ) -> Result<bool> {
        let mut submitted = self.submitted_prompts.subscribe();
        write_prompt(session, text).await?;
        let deadline = tokio::time::Instant::now() + TURN_RECEIPT_WAIT;
        loop {
            let got = match tokio::time::timeout_at(deadline, submitted.recv()).await {
                Ok(Ok((agent, got))) if &agent == id => got,
                Ok(Ok(_)) | Ok(Err(RecvError::Lagged(_))) => continue,
                Ok(Err(RecvError::Closed)) | Err(_) => return Ok(false),
            };
            if crate::prompt_history::holds(&got, text) {
                return Ok(true);
            }
            let Some(took) = crate::prompt_history::condense(&got) else {
                continue;
            };
            let start = |line: &str| line.chars().take(60).collect::<String>();
            bail!(
                "session {id} took only part of the text typed into it: its prompt starts \
                 \"{}…\" ({} bytes), the text typed starts \"{}…\" ({} bytes). It is working on \
                 that part now: send the whole text again, saying the last one was cut off",
                start(&took),
                got.len(),
                start(text),
                text.len()
            );
        }
    }
}

/// How long a prompt's Enter waits behind its text. Written straight
/// behind it, Claude Code takes the Enter for part of a paste and leaves
/// the prompt in its box unsent (2.1.287 and 2.1.319): a 250-character
/// line with the Enter straight behind it stayed in the box 2 times of 2,
/// and with the Enter 50 ms or 300 ms later it was submitted 4 of 4. The
/// TUI's composers wait the same.
const ENTER_PAUSE: Duration = Duration::from_millis(150);

/// Type `text` into `session` as its next turn: the text, then, on its
/// own, the Enter that sends it. Once the text is in, its Enter follows
/// whatever becomes of the caller: a send dropped with its client in
/// between would leave the text in the box, unsent.
pub(crate) async fn write_prompt(session: &Arc<crate::pty::PtySession>, text: &str) -> Result<()> {
    session.write_input(&nebula_core::turn::turn_bytes(text))?;
    let session = session.clone();
    tokio::spawn(async move {
        tokio::time::sleep(ENTER_PAUSE).await;
        session.write_input(b"\r")
    })
    .await?
}

fn agent_summary(
    projects: &HashMap<nebula_core::ProjectId, Project>,
    worktrees: &HashMap<nebula_core::WorktreeId, Worktree>,
    agent: Agent,
) -> Option<SessionSummary> {
    let worktree = worktrees.get(&agent.worktree_id)?;
    let project = projects.get(&worktree.project_id)?;
    let summary = agent
        .recent_prompts
        .last()
        .map(|p| p.text.clone())
        .unwrap_or_else(|| agent.name.clone());
    Some(SessionSummary {
        session: SessionRef::Agent(agent.id.clone()),
        id: agent.id.to_string(),
        name: agent.name,
        project_id: project.id.clone(),
        project: project.name.clone(),
        worktree_id: worktree.id.clone(),
        worktree: worktree.path.clone(),
        branch: worktree.branch.clone(),
        harness: agent.kind.as_str().to_string(),
        status: public_status(agent.status).to_string(),
        alive: agent.alive,
        summary,
    })
}

fn terminal_summary(
    projects: &HashMap<nebula_core::ProjectId, Project>,
    worktrees: &HashMap<nebula_core::WorktreeId, Worktree>,
    terminal: TerminalTab,
) -> Option<SessionSummary> {
    let worktree = worktrees.get(&terminal.worktree_id)?;
    let project = projects.get(&worktree.project_id)?;
    let summary = terminal
        .run_command
        .as_deref()
        .unwrap_or(&terminal.name)
        .to_string();
    Some(SessionSummary {
        session: SessionRef::Terminal(terminal.id.clone()),
        id: format!("terminal:{}", terminal.id),
        name: terminal.name,
        project_id: project.id.clone(),
        project: project.name.clone(),
        worktree_id: worktree.id.clone(),
        worktree: worktree.path.clone(),
        branch: worktree.branch.clone(),
        harness: "terminal".into(),
        status: if terminal.alive { "running" } else { "idle" }.into(),
        alive: terminal.alive,
        summary,
    })
}

fn public_status(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Fresh | AgentStatus::Finished => "idle",
        AgentStatus::Running => "running",
        AgentStatus::NeedsFeedback => "needs_feedback",
        AgentStatus::Terminated => "terminated",
        AgentStatus::Disconnected => "disconnected",
    }
}

fn is_idle(status: AgentStatus) -> bool {
    matches!(
        status,
        AgentStatus::Fresh
            | AgentStatus::Finished
            | AgentStatus::Terminated
            | AgentStatus::Disconnected
    )
}

fn resolve_in_rows<'a>(rows: &'a [SessionLookup], raw: &str) -> Result<&'a SessionLookup> {
    let needle = raw.to_ascii_lowercase();
    let exact = rows
        .iter()
        .filter(|row| {
            row.summary.id.eq_ignore_ascii_case(raw) || row.summary.name.eq_ignore_ascii_case(raw)
        })
        .collect::<Vec<_>>();
    match exact.as_slice() {
        [row] => return Ok(row),
        [] => {}
        _ => bail!("session name is ambiguous: {raw}; use an id"),
    }

    let fuzzy = rows
        .iter()
        .filter(|row| {
            [
                row.summary.id.as_str(),
                row.summary.name.as_str(),
                row.summary.project.as_str(),
                row.summary.branch.as_str(),
            ]
            .iter()
            .any(|field| field.to_ascii_lowercase().contains(&needle))
        })
        .collect::<Vec<_>>();
    match fuzzy.as_slice() {
        [row] => Ok(row),
        [] => bail!("session not found: {raw}"),
        _ => bail!("session match is ambiguous: {raw}; use an id"),
    }
}

fn transcript_context(path: &Path, max_lines: u32) -> Option<String> {
    let tail = crate::session_title::transcript_tail(path)?;
    let mut lines = Vec::new();
    for line in tail.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value.get("type").and_then(|v| v.as_str()) == Some("custom-title") {
            continue;
        }
        if let Some(rendered) = render_transcript_line(&value) {
            lines.push(rendered);
        }
    }
    let text = lines.join("\n");
    (!text.trim().is_empty()).then(|| limit_lines(&(text + "\n"), max_lines))
}

fn render_transcript_line(value: &serde_json::Value) -> Option<String> {
    let (role, content) = if let Some(message) = value.get("message") {
        (
            message.get("role").and_then(|v| v.as_str()),
            message.get("content"),
        )
    } else {
        (
            value
                .get("role")
                .and_then(|v| v.as_str())
                .or_else(|| value.get("type").and_then(|v| v.as_str())),
            value
                .get("content")
                .or_else(|| value.get("text"))
                .or_else(|| value.get("summary")),
        )
    };
    let role = role?;
    let text = content_text(content?)?;
    let role = match role {
        "user" => "User",
        "assistant" => "Assistant",
        "summary" => "Summary",
        other => other,
    };
    Some(format!("{role}: {}", single_line(&text)))
}

fn content_text(value: &serde_json::Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    if let Some(items) = value.as_array() {
        let parts = items
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .and_then(|v| v.as_str())
                    .or_else(|| item.get("content").and_then(|v| v.as_str()))
            })
            .collect::<Vec<_>>();
        if !parts.is_empty() {
            return Some(parts.join("\n"));
        }
    }
    None
}

fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn limit_lines(text: &str, max_lines: u32) -> String {
    let max = max_lines.max(1) as usize;
    let lines = text.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(max);
    let mut out = lines[start..].join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    last_bytes(&out, READ_MAX_BYTES).to_string()
}

/// The end of `text`, at most `max` bytes of it, cut on a character.
fn last_bytes(text: &str, max: usize) -> &str {
    let mut start = text.len().saturating_sub(max);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// The screen `session` shows now, as `nebula session read` prints it:
/// its output laid out on a screen the PTY's size, never the bytes with
/// their escapes dropped, which lose the spaces a CLI moves the cursor
/// over and keep every frame it has since drawn over.
fn screen_text(session: &crate::pty::PtySession) -> String {
    let mut text = session.screen_rows().join("\n");
    text.push('\n');
    text
}

/// The text of the newest assistant message in the Claude transcript at
/// `path` past its byte `from`: each of its text blocks, which Claude
/// writes on lines of their own under the message's id, in order. `None`
/// when no such message is there yet.
fn newest_reply(path: &Path, from: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(
        from.max(len.saturating_sub(REPLY_SCAN_BYTES)),
    ))
    .ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let mut newest: Option<(Option<String>, Vec<String>)> = None;
    for line in String::from_utf8_lossy(&bytes).lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(message) = value.get("message") else {
            continue;
        };
        if message.get("role").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let texts: Vec<String> = match message.get("content") {
            Some(serde_json::Value::String(text)) => vec![text.clone()],
            Some(serde_json::Value::Array(items)) => items
                .iter()
                .filter(|item| item.get("type").and_then(|v| v.as_str()) == Some("text"))
                .filter_map(|item| item.get("text")?.as_str().map(str::to_string))
                .collect(),
            _ => Vec::new(),
        };
        if texts.iter().all(|t| t.trim().is_empty()) {
            continue;
        }
        let id = message
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        match &mut newest {
            Some((last, parts)) if id.is_some() && *last == id => parts.extend(texts),
            _ => newest = Some((id, texts)),
        }
    }
    let text = newest?.1.join("\n");
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn labelled_question(caller_name: &str, question: &str) -> Result<String> {
    let question = validate_question(question)?;
    Ok(format!(
        "[question from session \"{}\"]\n{}\n\nAnswer concisely; your reply is returned to the asking session.",
        caller_name.trim(),
        question
    ))
}

fn validate_question(question: &str) -> Result<&str> {
    let text = question.trim();
    if text.is_empty() {
        bail!("question is empty");
    }
    if text.contains('\0') {
        bail!("question cannot contain NUL bytes");
    }
    if text.len() > MAX_CLOUD_PROMPT_BYTES {
        bail!(
            "question is too long (max {} KiB)",
            MAX_CLOUD_PROMPT_BYTES / 1024
        );
    }
    Ok(text)
}

async fn wait_until_idle(
    rx: &mut tokio::sync::broadcast::Receiver<ServerEvent>,
    target: &AgentId,
    deadline: tokio::time::Instant,
) -> Result<()> {
    loop {
        let event = recv_until(rx, deadline).await?;
        if let ServerEvent::StatusChanged { agent, status, .. } = event {
            if &agent == target && is_idle(status) {
                return Ok(());
            }
        }
    }
}

async fn wait_for_answer_turn(
    rx: &mut tokio::sync::broadcast::Receiver<ServerEvent>,
    target: &AgentId,
    deadline: tokio::time::Instant,
) -> Result<()> {
    let mut saw_activity = false;
    loop {
        let event = recv_until(rx, deadline).await?;
        match event {
            ServerEvent::StatusChanged { agent, status, .. } if agent == *target => {
                if matches!(status, AgentStatus::Running | AgentStatus::NeedsFeedback) {
                    saw_activity = true;
                } else if saw_activity && is_idle(status) {
                    return Ok(());
                }
            }
            ServerEvent::SessionExited {
                session: SessionRef::Agent(agent),
                ..
            } if agent == *target => return Ok(()),
            _ => {}
        }
    }
}

async fn recv_until(
    rx: &mut tokio::sync::broadcast::Receiver<ServerEvent>,
    deadline: tokio::time::Instant,
) -> Result<ServerEvent> {
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            bail!("timed out waiting for the target session");
        }
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Ok(event)) => return Ok(event),
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                bail!("daemon event stream closed")
            }
            Err(_) => bail!("timed out waiting for the target session"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::HookEnv;
    use crate::store::Store;
    use nebula_core::{ProjectId, TerminalId, WorktreeId};

    fn daemon() -> Arc<Daemon> {
        let daemon = Daemon::new(
            Arc::new(Store::open_in_memory().unwrap()),
            HookEnv {
                port: 0,
                token: String::new(),
            },
        );
        daemon
            .store
            .insert_project(&Project {
                id: ProjectId("p".into()),
                name: "nebula".into(),
                repo_path: "/tmp/nebula".into(),
                sort_order: 0,
            })
            .unwrap();
        daemon
            .store
            .insert_worktree(&Worktree {
                id: WorktreeId("w".into()),
                project_id: ProjectId("p".into()),
                path: "/tmp/nebula".into(),
                branch: "main".into(),
                is_main: true,
                sort_order: 0,
                removed: false,
            })
            .unwrap();
        daemon
    }

    fn agent(id: &str, name: &str, status: AgentStatus) -> Agent {
        Agent {
            id: AgentId(id.into()),
            worktree_id: WorktreeId("w".into()),
            name: name.into(),
            status,
            archived: false,
            archived_at: 0,
            unseen: false,
            status_changed_at: 0,
            kind: nebula_core::AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: Some("sid".into()),
            cloud_session_id: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
            sort_order: 0,
            alive: false,
            recent_prompts: Vec::new(),
        }
    }

    #[test]
    fn list_output_includes_project_worktree_harness_status_and_summary() {
        let daemon = daemon();
        let mut a = agent("a1", "research", AgentStatus::Finished);
        a.recent_prompts.push(nebula_core::PromptEntry {
            text: "which json parser?".into(),
            submitted_at: 7,
        });
        daemon.store.insert_agent(&a).unwrap();
        daemon
            .store
            .push_prompt(&a.id, a.recent_prompts.last().unwrap())
            .unwrap();
        daemon
            .store
            .insert_terminal(&TerminalTab {
                id: TerminalId("t1".into()),
                worktree_id: WorktreeId("w".into()),
                name: "shell".into(),
                sort_order: 0,
                alive: false,
                run_command: None,
            })
            .unwrap();

        let rows = daemon.session_summaries().unwrap();
        assert!(rows.iter().any(|r| {
            r.name == "research"
                && r.project == "nebula"
                && r.branch == "main"
                && r.harness == "claude"
                && r.status == "idle"
                && r.summary == "which json parser?"
        }));
        assert!(rows
            .iter()
            .any(|r| r.name == "shell" && r.harness == "terminal"));
    }

    // A session done for longer than the settle is done at once; one still
    // working is not; an archived or deleted one is the error.
    #[tokio::test]
    async fn await_done_answers_a_settled_row_and_refuses_one_that_is_gone() {
        let daemon = daemon();
        let mut done = agent("a1", "done", AgentStatus::Finished);
        done.status_changed_at = nebula_core::clock::now_ms() - 60_000;
        daemon.store.insert_agent(&done).unwrap();
        let status = tokio::time::timeout(Duration::from_secs(2), daemon.await_done(&done.id))
            .await
            .expect("a long-finished row is done at once");
        assert_eq!(status.unwrap(), AgentStatus::Finished);

        let mut working = agent("a2", "working", AgentStatus::Running);
        working.status_changed_at = done.status_changed_at;
        daemon.store.insert_agent(&working).unwrap();
        let waited =
            tokio::time::timeout(Duration::from_secs(1), daemon.await_done(&working.id)).await;
        assert!(waited.is_err(), "a running row is not done: {waited:?}");

        let mut archived = agent("a3", "archived", AgentStatus::Finished);
        archived.archived = true;
        daemon.store.insert_agent(&archived).unwrap();
        let err = daemon.await_done(&archived.id).await.unwrap_err();
        assert!(err.to_string().contains("is archived"), "{err}");
        let err = daemon
            .await_done(&AgentId("gone".into()))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("was deleted"), "{err}");
    }

    #[test]
    fn fuzzy_resolution_rejects_ambiguous_names() {
        let daemon = daemon();
        daemon
            .store
            .insert_agent(&agent("a1", "research alpha", AgentStatus::Finished))
            .unwrap();
        daemon
            .store
            .insert_agent(&agent("a2", "research beta", AgentStatus::Finished))
            .unwrap();

        assert!(daemon
            .resolve_session_target("a1")
            .is_ok_and(|s| matches!(s, SessionRef::Agent(id) if id.0 == "a1")));
        let err = daemon.resolve_session_target("research").unwrap_err();
        assert!(err.to_string().contains("ambiguous"));
    }

    #[test]
    fn transcript_reading_renders_clean_capped_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sid.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"type":"user","message":{"role":"user","content":"first"}}"#,
                "\n",
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"second"}]}}"#,
                "\n",
                r#"{"type":"user","message":{"role":"user","content":"third"}}"#,
                "\n",
            ),
        )
        .unwrap();
        let text = transcript_context(&path, 2).unwrap();
        assert_eq!(text, "Assistant: second\nUser: third\n");
    }

    /// A merged branch's workers are read after the fact: an archived row
    /// from its transcript on disk, with no hook since the daemon started,
    /// and a deleted one (its checkout cleaned up) from the transcript its
    /// hooks named. Only by exact id: a name never reaches them.
    #[test]
    fn read_finds_an_archived_or_deleted_session_by_its_id() {
        let daemon = daemon();
        let dir = tempfile::tempdir().unwrap();
        let slug = dir.path().join("projects").join("-tmp-nebula");
        std::fs::create_dir_all(&slug).unwrap();
        let reply = |text: &str| {
            format!(
                r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{text}"}}]}}}}"#
            ) + "\n"
        };
        std::fs::write(slug.join("sid-gone.jsonl"), reply("DONE fixed the loop")).unwrap();
        std::fs::write(slug.join("sid-old.jsonl"), reply("DONE sorted it")).unwrap();
        daemon.note_transcript(
            &AgentId("gone".into()),
            crate::session_title::TranscriptRef {
                transcript_path: slug.join("sid-gone.jsonl"),
                session_id: "sid-gone".into(),
            },
        );
        let mut old = agent("old", "changelog", AgentStatus::Finished);
        old.archived = true;
        old.session_id = Some("sid-old".into());
        daemon.store.insert_agent(&old).unwrap();

        let read = daemon.read_session_context("old", 5).unwrap();
        assert_eq!(read.text, "Assistant: DONE sorted it\n");
        let read = daemon.read_session_context("gone", 5).unwrap();
        assert_eq!(read.text, "Assistant: DONE fixed the loop\n");
        let err = daemon.read_session_context("changelog", 5).unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
    }

    /// A prompt's Enter reaches the CLI on its own, after its text: written
    /// straight behind it, Claude Code takes it for part of a paste and
    /// leaves the prompt unsent. A stand-in in raw mode reads once for the
    /// text and once more for the Enter.
    #[tokio::test]
    async fn a_prompts_enter_arrives_on_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let at = |name: &str| dir.path().join(name);
        let script = format!(
            "stty raw -echo; touch '{ready}'; dd bs=4096 count=1 of='{first}' 2>/dev/null; \
             dd bs=4096 count=1 of='{second}' 2>/dev/null; touch '{done}'; exec sleep 60",
            ready = at("ready").display(),
            first = at("first").display(),
            second = at("second").display(),
            done = at("done").display(),
        );
        let session = crate::pty::PtySession::spawn(
            SessionRef::Agent(AgentId("a".into())),
            crate::pty::SpawnSpec {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), script],
                cwd: dir.path().into(),
                env: Vec::new(),
                scrub_env: &[],
                cols: 80,
                rows: 24,
            },
        )
        .unwrap();
        let appears = |name: &'static str| {
            let path = at(name);
            async move {
                let deadline = std::time::Instant::now() + Duration::from_secs(10);
                while !path.exists() {
                    assert!(std::time::Instant::now() < deadline, "no {name}");
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }
        };
        appears("ready").await;
        write_prompt(&session, "fix the login loop").await.unwrap();
        appears("done").await;
        assert_eq!(std::fs::read(at("first")).unwrap(), b"fix the login loop");
        assert_eq!(std::fs::read(at("second")).unwrap(), b"\r");
        session.kill();
    }

    /// A typed turn is confirmed by the session's own report of the prompt
    /// it took, whole, however its CLI wrapped it; another session's
    /// prompt and one the CLI injected are no answer, and a report of only
    /// its end, as Claude Code made of a 1,536-byte line, is the error.
    #[tokio::test]
    async fn a_typed_turn_is_confirmed_only_by_its_whole_prompt() {
        let daemon = daemon();
        let id = AgentId("a1".into());
        let dir = tempfile::tempdir().unwrap();
        let session = crate::pty::PtySession::spawn(
            SessionRef::Agent(id.clone()),
            crate::pty::SpawnSpec {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), "stty raw -echo; exec cat >/dev/null".into()],
                cwd: dir.path().into(),
                env: Vec::new(),
                scrub_env: &[],
                cols: 80,
                rows: 24,
            },
        )
        .unwrap();
        let typed = "Fix the login redirect. Then add a test that signs in twice, and update \
                     the docs for the new flow.";
        let report = |agent: &str, text: &str| {
            daemon.prompt_submitted(&AgentId(agent.into()), text.to_string());
        };
        let (confirmed, ()) = tokio::join!(daemon.type_turn(&id, &session, typed), async {
            report("a2", typed);
            report("a1", "<task-notification> <task-id>b1</task-id>");
            report(
                "a1",
                &format!(
                    "\n\n<pasted_content id=\"a4bc\">\n{typed}\n</pasted_content id=\"a4bc\">\n"
                ),
            );
        });
        assert!(confirmed.unwrap());

        let (cut, ()) = tokio::join!(daemon.type_turn(&id, &session, typed), async {
            report("a1", "e the docs for the new flow.");
        });
        let err = cut.unwrap_err().to_string();
        assert!(
            err.contains("took only part")
                && err.contains("starts \"e the docs for the new flow.…\""),
            "{err}"
        );
        session.kill();
    }

    #[test]
    fn lines_are_capped_from_the_end_on_a_character() {
        assert_eq!(limit_lines("one\ntwo\nthree", 2), "two\nthree\n");
        assert_eq!(last_bytes("aé", 1), "");
        assert_eq!(last_bytes("aé", 2), "é");
    }

    /// An ask's answer is the newest assistant message the turn wrote, all
    /// of its text blocks: not an earlier one of the turn, a tool call or a
    /// tool's result, nor anything from before the question.
    #[test]
    fn the_reply_is_the_newest_assistant_message_past_the_mark() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sid.jsonl");
        let assistant = |id: &str, block: &str| {
            format!(
                r#"{{"type":"assistant","message":{{"id":"{id}","role":"assistant","content":[{block}]}}}}"#
            ) + "\n"
        };
        let text = |t: &str| format!(r#"{{"type":"text","text":"{t}"}}"#);
        let before = assistant("m0", &text("an old answer"));
        std::fs::write(&path, &before).unwrap();
        let mark = before.len() as u64;
        assert_eq!(newest_reply(&path, mark), None, "nothing past the mark yet");
        let turn = [
            r#"{"type":"user","message":{"role":"user","content":"[question from session \"captain\"]\nwhich parser?"}}"#.to_string() + "\n",
            assistant("m1", &text("Let me look.")),
            assistant("m1", r#"{"type":"tool_use","id":"t","name":"Read","input":{}}"#),
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"fn main"}]}}"#.to_string() + "\n",
            assistant("m2", &text("It uses serde_json.")),
            assistant("m2", &text("DONE answered")),
        ]
        .concat();
        std::fs::write(&path, before + &turn).unwrap();
        assert_eq!(
            newest_reply(&path, mark).as_deref(),
            Some("It uses serde_json.\nDONE answered")
        );
        assert_eq!(
            newest_reply(&path, 0).as_deref(),
            Some("It uses serde_json.\nDONE answered")
        );
    }

    /// A session with no transcript reads as its screen: a CLI moves the
    /// cursor over the spaces it does not repaint and draws over what it
    /// drew, which stripping the escapes off its bytes gets wrong.
    #[tokio::test]
    async fn a_session_without_a_transcript_reads_as_its_screen() {
        let daemon = daemon();
        let sref = SessionRef::Terminal(TerminalId("t1".into()));
        let session = crate::pty::PtySession::spawn(
            sref.clone(),
            crate::pty::SpawnSpec {
                program: "/bin/sh".into(),
                args: vec![
                    "-c".into(),
                    "printf 'Answer\\033[1Cconcisely\\r\\nworking\\rdone   \\r\\n'; exec sleep 60"
                        .into(),
                ],
                cwd: std::env::temp_dir(),
                env: Vec::new(),
                scrub_env: &[],
                cols: 40,
                rows: 6,
            },
        )
        .unwrap();
        daemon.install_session(session.clone());
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !session.screen_rows().iter().any(|r| r == "done") {
            assert!(
                std::time::Instant::now() < deadline,
                "{:?}",
                session.screen_rows()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let tail = daemon.read_output_tail_clean(&sref, 8192).unwrap();
        assert_eq!(
            String::from_utf8(tail.data).unwrap(),
            "Answer concisely\ndone\n"
        );
        assert_eq!((tail.cols, tail.rows), (40, 6));
        assert_eq!(
            daemon.read_session_ref(&sref, 1).unwrap(),
            "done\n",
            "nebula read's fallback lays it out too"
        );
        session.kill();
    }
}
