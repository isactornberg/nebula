//! A subscribed connection to the running DAEMON that keeps a [`Tree`]
//! current from the events it reads: what a one-shot command and a server
//! in front of the daemon both drive the tree through.

use std::time::Duration;

use anyhow::{bail, Result};
use nebula_core::codec::write_frame;
use nebula_core::{
    paths, AgentId, ClientRequest, EntityId, OutputTail, ProjectId, ServerEvent, SessionRef,
    RANK_PROTOCOL, SHOW_PROTOCOL, START_ISSUE_PROTOCOL,
};
use tokio::net::unix::OwnedWriteHalf;
use tokio::sync::mpsc::Receiver;
use tokio::time::Instant;

use crate::connection;
use crate::Tree;

/// A subscribed connection to the running daemon, and the tree it
/// described, kept current by every event read through it.
pub struct Client {
    writer: OwnedWriteHalf,
    events: Receiver<ServerEvent>,
    pub tree: Tree,
    /// The protocol the daemon speaks, so a request it would not know is
    /// never sent to it.
    pub protocol: u32,
    last_req: u64,
}

/// The daemon hung up, [`connection::CLOSED_BEFORE_REPLY`]: an error a
/// caller can tell from a refusal (`err.is::<Closed>()`). An IN-PLACE
/// RESTART drops every connection, and a wait carries on over a new one.
#[derive(Debug)]
pub struct Closed;

impl std::fmt::Display for Closed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(connection::CLOSED_BEFORE_REPLY)
    }
}

impl std::error::Error for Closed {}

impl Client {
    /// Connect, subscribe, and take the Snapshot. Never spawns a daemon:
    /// with none running the error ends in `undone`, what the command
    /// therefore did not do.
    pub async fn connect(undone: &str) -> Result<Self> {
        let Ok(stream) = connection::try_connect(&paths::socket_path()).await else {
            bail!("no nebula daemon is running: {undone}");
        };
        let connection = connection::handshake(stream).await?;
        let (read_half, writer) = connection.stream.into_split();
        let mut client = Self {
            writer,
            events: connection::spawn_reader(read_half),
            tree: Tree::default(),
            protocol: connection.protocol_version,
            last_req: 0,
        };
        client.send(&ClientRequest::Subscribe).await?;
        while !matches!(client.next_event().await?, ServerEvent::Snapshot { .. }) {}
        Ok(client)
    }

    /// Write one request. It has reached the socket when this returns, so
    /// a request the daemon never answers is still safe to exit after.
    pub async fn send(&mut self, request: &ClientRequest) -> Result<()> {
        write_frame(&mut self.writer, request)
            .await
            .map_err(|_| Closed.into())
    }

    /// A req id no other request on this connection carries.
    pub fn next_req_id(&mut self) -> u64 {
        self.last_req += 1;
        self.last_req
    }

    /// The daemon's next event, folded into the tree before it is handed
    /// over. Safe to give up on (a timeout, a `select!`): an event is
    /// either still queued or already folded.
    pub async fn next_event(&mut self) -> Result<ServerEvent> {
        let event = self.events.recv().await.ok_or(Closed)?;
        self.tree.apply(&event);
        Ok(event)
    }

    /// Send the request `build` makes of a fresh req id and read on to its
    /// Ack, returning what that created. The daemon's refusal is the error.
    pub async fn request(
        &mut self,
        build: impl FnOnce(u64) -> ClientRequest,
    ) -> Result<Option<EntityId>> {
        let req_id = self.next_req_id();
        self.send(&build(req_id)).await?;
        loop {
            match self.next_event().await? {
                ServerEvent::Ack { req_id: r, created } if r == req_id => return Ok(created),
                ServerEvent::Error {
                    req_id: Some(r),
                    message,
                } if r == req_id => bail!("{message}"),
                _ => {}
            }
        }
    }

    /// The end of session `agent`'s ring, as much as a screen takes
    /// ([`SCREEN_TAIL_BYTES`](crate::screen::SCREEN_TAIL_BYTES)), or `None`
    /// when it has no live PTY. With `after_seq`, no bytes when the ring has
    /// not grown past it. `TailOutput` is answered by an `OutputTail`, never
    /// an Ack.
    pub async fn tail(
        &mut self,
        agent: &AgentId,
        after_seq: Option<u64>,
    ) -> Result<Option<OutputTail>> {
        let req_id = self.next_req_id();
        self.send(&ClientRequest::TailOutput {
            req_id,
            session: SessionRef::Agent(agent.clone()),
            max_bytes: crate::screen::SCREEN_TAIL_BYTES,
            after_seq,
        })
        .await?;
        loop {
            match self.next_event().await? {
                ServerEvent::OutputTail {
                    req_id: r, tail, ..
                } if r == req_id => return Ok(tail),
                ServerEvent::Error {
                    req_id: Some(r),
                    message,
                } if r == req_id => bail!("{message}"),
                _ => {}
            }
        }
    }

    /// Every TUI puts session `id` up: the web app's **Open in terminal**.
    /// The error is the daemon's refusal, when no TUI is open to show it
    /// ([`Tree::tui_open`] says so beforehand) or the session is gone or
    /// archived, or that the daemon is too old to be asked.
    pub async fn show_session(&mut self, id: &AgentId) -> Result<()> {
        if self.protocol < SHOW_PROTOCOL {
            bail!("the daemon is too old to show a session in the TUI: `nebula reload` restarts it on this build");
        }
        self.request(|req_id| ClientRequest::ShowSession {
            req_id,
            id: id.clone(),
        })
        .await
        .map(|_| ())
    }

    /// The web app's **Start**: hand the issue at `url` (`key`, `title`)
    /// to the CAPTAIN of `project`, in a brief to start a worker on `task`.
    /// The captain it went to, and whether the brief is held until the
    /// captain is free. The error is the daemon's refusal, or that it is too
    /// old to be asked.
    pub async fn start_issue(
        &mut self,
        project: &ProjectId,
        url: &str,
        key: &str,
        title: &str,
        task: &str,
    ) -> Result<(AgentId, bool)> {
        if self.protocol < START_ISSUE_PROTOCOL {
            bail!("the running daemon cannot hand an issue to a captain: `nebula reload` moves it onto this build, its sessions kept");
        }
        let req_id = self.next_req_id();
        self.send(&ClientRequest::StartIssue {
            req_id,
            project: project.clone(),
            issue_url: url.to_string(),
            key: key.to_string(),
            title: title.to_string(),
            task: task.to_string(),
        })
        .await?;
        loop {
            match self.next_event().await? {
                ServerEvent::IssueStarted {
                    req_id: r,
                    captain,
                    held,
                } if r == req_id => return Ok((captain, held)),
                ServerEvent::Error {
                    req_id: Some(r),
                    message,
                } if r == req_id => bail!("{message}"),
                _ => {}
            }
        }
    }

    /// The web app's ask that the ORCHESTRATOR rank the focus picks: the
    /// RANK TURN held until it is at rest with an empty input box, or, not
    /// to `wait`, typed only when it is now. The error is the daemon's
    /// refusal, or that it is too old to be asked.
    pub async fn rank_focus(&mut self, wait: bool) -> Result<()> {
        if self.protocol < RANK_PROTOCOL {
            bail!("the running daemon cannot ask the orchestrator to rank: `nebula reload` moves it onto this build, its sessions kept");
        }
        self.request(|req_id| ClientRequest::RankFocus { req_id, wait })
            .await
            .map(|_| ())
    }

    /// Add the SCHEDULE `spec`: its id. The error is the daemon's refusal,
    /// or that it is too old to be asked.
    pub async fn add_schedule(&mut self, spec: nebula_core::ScheduleSpec) -> Result<String> {
        self.schedules_known()?;
        let req_id = self.next_req_id();
        self.send(&ClientRequest::AddSchedule { req_id, spec })
            .await?;
        loop {
            match self.next_event().await? {
                ServerEvent::ScheduleAdded { req_id: r, id } if r == req_id => return Ok(id),
                ServerEvent::Error {
                    req_id: Some(r),
                    message,
                } if r == req_id => bail!("{message}"),
                _ => {}
            }
        }
    }

    /// Remove the schedule `id`.
    pub async fn remove_schedule(&mut self, id: &str) -> Result<()> {
        self.schedules_known()?;
        self.request(|req_id| ClientRequest::RemoveSchedule {
            req_id,
            id: id.to_string(),
        })
        .await
        .map(|_| ())
    }

    /// Run the schedule `id` now: the run, as it stands once it has done
    /// what it does at once.
    pub async fn run_schedule(&mut self, id: &str) -> Result<nebula_core::ScheduleRun> {
        self.schedules_known()?;
        let req_id = self.next_req_id();
        self.send(&ClientRequest::RunSchedule {
            req_id,
            id: id.to_string(),
        })
        .await?;
        loop {
            match self.next_event().await? {
                ServerEvent::ScheduleRan { req_id: r, run } if r == req_id => return Ok(run),
                ServerEvent::Error {
                    req_id: Some(r),
                    message,
                } if r == req_id => bail!("{message}"),
                _ => {}
            }
        }
    }

    /// The SCHEDULES and the run log, once the daemon has said them, which
    /// it does right after the Snapshot and after every change.
    pub async fn schedules(&mut self) -> Result<&nebula_core::ScheduleBook> {
        self.schedules_known()?;
        let wait = std::time::Duration::from_secs(5);
        tokio::time::timeout(wait, async {
            while !matches!(self.next_event().await?, ServerEvent::Schedules { .. }) {}
            anyhow::Ok(())
        })
        .await
        .map_err(|_| anyhow::anyhow!("the daemon never said its schedules"))??;
        Ok(&self.tree.schedules)
    }

    /// Refused against a daemon older than schedules.
    fn schedules_known(&self) -> Result<()> {
        if self.protocol < nebula_core::SCHEDULE_PROTOCOL {
            bail!("the running daemon keeps no schedules: `nebula reload` moves it onto this build, its sessions kept");
        }
        Ok(())
    }

    /// Send the request `build` makes of a fresh req id, one that types a
    /// turn (`TypeTurn`, `OrchestratorSend`), and read on to its answer:
    /// whether the session reported taking the whole of it. Its taking only
    /// part is the daemon's refusal, and the error.
    pub async fn type_turn(&mut self, build: impl FnOnce(u64) -> ClientRequest) -> Result<bool> {
        let req_id = self.next_req_id();
        self.send(&build(req_id)).await?;
        loop {
            match self.next_event().await? {
                ServerEvent::TurnTyped {
                    req_id: r,
                    confirmed,
                } if r == req_id => return Ok(confirmed),
                ServerEvent::Error {
                    req_id: Some(r),
                    message,
                } if r == req_id => bail!("{message}"),
                _ => {}
            }
        }
    }

    /// Read events until `check` finds what it is after in the tree;
    /// `None` when `timeout` runs out first. With no timeout (or one too
    /// long for the clock to hold) it reads for as long as that takes.
    pub async fn wait_for<T>(
        &mut self,
        timeout: Option<Duration>,
        check: impl Fn(&Tree) -> Option<T>,
    ) -> Result<Option<T>> {
        let deadline = timeout.and_then(|t| Instant::now().checked_add(t));
        self.wait_until(deadline, check).await
    }

    /// [`Self::wait_for`] to a `deadline` (`None`: none).
    pub async fn wait_until<T>(
        &mut self,
        deadline: Option<Instant>,
        check: impl Fn(&Tree) -> Option<T>,
    ) -> Result<Option<T>> {
        loop {
            if let Some(found) = check(&self.tree) {
                return Ok(Some(found));
            }
            if self.next_event_before(deadline).await?.is_none() {
                return Ok(None);
            }
        }
    }

    /// [`Self::next_event`], or `None` once `deadline` (if any) passes
    /// first.
    pub async fn next_event_before(
        &mut self,
        deadline: Option<Instant>,
    ) -> Result<Option<ServerEvent>> {
        match deadline {
            Some(deadline) => match tokio::time::timeout_at(deadline, self.next_event()).await {
                Ok(event) => event.map(Some),
                Err(_) => Ok(None),
            },
            None => self.next_event().await.map(Some),
        }
    }
}
