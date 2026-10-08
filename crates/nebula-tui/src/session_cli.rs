//! One-shot clients that drive the tree from any shell: `nebula tree`,
//! which prints it, and `nebula session`, which starts a session in it and
//! follows one from there. Each is composed from requests the daemon
//! already answers (`Subscribe` for the Snapshot, then what the TUI sends
//! too: `CreateAgent`, `TailOutput`, `Input`, `RestartAgent`,
//! `DeleteAgent`), so a script gets sessions that sit on the grid like any
//! other. None of them starts a daemon: with none running there is no tree
//! to read and nothing to put a session in.
//!
//! This module is what they share: the connection, and the tree it keeps.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use nebula_core::codec::write_frame;
use nebula_core::{paths, Agent, ClientRequest, Entity, EntityId, ServerEvent};
use tokio::net::unix::OwnedWriteHalf;
use tokio::sync::mpsc::Receiver;

use crate::app::Tree;
use crate::event_loop::upsert_by;
use crate::ipc;

mod session;
mod start;
mod tree;

pub use session::{run, SessionOp};
pub use start::StartOpts;
pub use tree::print_tree;

/// A subscribed connection to the running daemon, and the tree it
/// described, kept current by every event read through it.
pub(crate) struct Client {
    writer: OwnedWriteHalf,
    events: Receiver<ServerEvent>,
    pub(crate) tree: Tree,
    last_req: u64,
}

impl Client {
    /// Connect, subscribe, and take the Snapshot. Never spawns a daemon:
    /// with none running the error ends in `undone`, what the command
    /// therefore did not do.
    pub(crate) async fn connect(undone: &str) -> Result<Self> {
        let Ok(stream) = ipc::try_connect(&paths::socket_path()).await else {
            bail!("no nebula daemon is running: {undone}");
        };
        let (read_half, writer) = ipc::handshake(stream).await?.stream.into_split();
        let mut client = Self {
            writer,
            events: ipc::spawn_reader(read_half),
            tree: Tree::default(),
            last_req: 0,
        };
        client.send(&ClientRequest::Subscribe).await?;
        while !matches!(client.next_event().await?, ServerEvent::Snapshot { .. }) {}
        Ok(client)
    }

    /// Write one request. It has reached the socket when this returns, so
    /// a request the daemon never answers is still safe to exit after.
    pub(crate) async fn send(&mut self, request: &ClientRequest) -> Result<()> {
        Ok(write_frame(&mut self.writer, request).await?)
    }

    /// A req id no other request on this connection carries.
    pub(crate) fn next_req_id(&mut self) -> u64 {
        self.last_req += 1;
        self.last_req
    }

    /// The daemon's next event, folded into the tree before it is handed
    /// over. Safe to give up on (a timeout, a `select!`): an event is
    /// either still queued or already folded.
    pub(crate) async fn next_event(&mut self) -> Result<ServerEvent> {
        let event = self.events.recv().await.context(ipc::CLOSED_BEFORE_REPLY)?;
        fold(&mut self.tree, &event);
        Ok(event)
    }

    /// Send the request `build` makes of a fresh req id and read on to its
    /// Ack, returning what that created. The daemon's refusal is the error.
    pub(crate) async fn request(
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

    /// Read events until `check` finds what it is after in the tree;
    /// `None` when `timeout` runs out first. With no timeout (or one too
    /// long for the clock to hold) it reads for as long as that takes.
    pub(crate) async fn wait_for<T>(
        &mut self,
        timeout: Option<Duration>,
        check: impl Fn(&Tree) -> Option<T>,
    ) -> Result<Option<T>> {
        let deadline = timeout.and_then(|t| tokio::time::Instant::now().checked_add(t));
        loop {
            if let Some(found) = check(&self.tree) {
                return Ok(Some(found));
            }
            match deadline {
                Some(deadline) => {
                    match tokio::time::timeout_at(deadline, self.next_event()).await {
                        Ok(event) => event?,
                        Err(_) => return Ok(None),
                    }
                }
                None => self.next_event().await?,
            };
        }
    }
}

/// Fold one event into the mirror, as the TUI does into its own: a
/// Snapshot replaces it, an upsert lands by id, a removal takes the row and
/// everything under it (the daemon cascades without saying so row by row).
/// Terminals and links are left out: no command here reads one.
fn fold(tree: &mut Tree, event: &ServerEvent) {
    match event {
        ServerEvent::Snapshot {
            projects,
            worktrees,
            agents,
            ..
        } => {
            *tree = Tree {
                projects: projects.clone(),
                worktrees: worktrees.clone(),
                agents: agents.clone(),
                ..Tree::default()
            };
        }
        ServerEvent::EntityUpserted { entity } => match entity.clone() {
            Entity::Project(p) => upsert_by(&mut tree.projects, p, |x, y| x.id == y.id),
            Entity::Worktree(w) => upsert_by(&mut tree.worktrees, w, |x, y| x.id == y.id),
            Entity::Agent(a) => upsert_by(&mut tree.agents, a, |x, y| x.id == y.id),
            Entity::Terminal(_) | Entity::Link(_) => {}
        },
        ServerEvent::EntityRemoved { id } => {
            match id {
                EntityId::Project(id) => tree.projects.retain(|p| &p.id != id),
                EntityId::Worktree(id) => tree.worktrees.retain(|w| &w.id != id),
                EntityId::Agent(id) => tree.agents.retain(|a| &a.id != id),
                EntityId::Terminal(_) | EntityId::Link(_) => {}
            }
            let Tree {
                projects,
                worktrees,
                agents,
                ..
            } = tree;
            worktrees.retain(|w| projects.iter().any(|p| p.id == w.project_id));
            agents.retain(|a| worktrees.iter().any(|w| w.id == a.worktree_id));
        }
        ServerEvent::StatusChanged {
            agent,
            status,
            changed_at,
            unseen,
        } => {
            if let Some(row) = tree.agents.iter_mut().find(|a| &a.id == agent) {
                row.status = *status;
                row.status_changed_at = *changed_at;
                row.unseen = *unseen;
            }
        }
        _ => {}
    }
}

/// The session with this id, as `nebula tree` prints it.
pub(crate) fn session_by_id<'a>(tree: &'a Tree, id: &str) -> Result<&'a Agent> {
    tree.agents
        .iter()
        .find(|a| a.id.as_str() == id)
        .with_context(|| format!("no session {id}: `nebula tree` lists them"))
}

/// `name` with its control characters replaced: a session can be renamed
/// to anything, and an escape sequence in a name must not reach the
/// terminal this is printed on.
pub(crate) fn printable(name: &str) -> String {
    name.replace(char::is_control, "?")
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::{AgentId, AgentKind, AgentStatus, Project, ProjectId, Worktree, WorktreeId};
    use std::path::Path;

    pub(super) fn worktree(id: &str, path: &Path, is_main: bool) -> Worktree {
        Worktree {
            id: WorktreeId(id.into()),
            project_id: ProjectId("p".into()),
            path: path.to_path_buf(),
            branch: id.into(),
            is_main,
            sort_order: 0,
        }
    }

    pub(super) fn project(path: &Path) -> Project {
        Project {
            id: ProjectId("p".into()),
            name: "orbit".into(),
            repo_path: path.to_path_buf(),
            sort_order: 0,
            space: None,
        }
    }

    pub(super) fn agent(id: &str, worktree: &str, name: &str) -> Agent {
        Agent {
            id: AgentId(id.into()),
            worktree_id: WorktreeId(worktree.into()),
            name: name.into(),
            status: AgentStatus::Running,
            archived: false,
            archived_at: 0,
            unseen: false,
            status_changed_at: 0,
            kind: AgentKind::Claude,
            custom_harness: None,
            model: Some("opus".into()),
            effort: Some("high".into()),
            session_id: Some("s1".into()),
            cloud_session_id: None,
            issue_url: None,
            sort_order: 0,
            alive: true,
            recent_prompts: Vec::new(),
        }
    }

    /// One project: the root checkout with a session and an archived one,
    /// and an empty linked checkout.
    pub(super) fn sample() -> Tree {
        let mut archived = agent("a2", "main", "Old Work");
        archived.archived = true;
        Tree {
            projects: vec![project(Path::new("/code/orbit"))],
            worktrees: vec![
                worktree("main", Path::new("/code/orbit"), true),
                worktree("feat", Path::new("/code/orbit-worktrees/feat"), false),
            ],
            agents: vec![agent("a1", "main", "Fix Login"), archived],
            ..Tree::default()
        }
    }

    #[test]
    fn a_session_is_found_by_its_id_or_the_error_names_nebula_tree() {
        let tree = sample();
        assert_eq!(session_by_id(&tree, "a1").unwrap().name, "Fix Login");
        let err = session_by_id(&tree, "nope").unwrap_err().to_string();
        assert!(
            err.contains("nope") && err.contains("`nebula tree`"),
            "{err}"
        );
    }

    // The mirror follows the daemon's deltas: an upsert lands by id, a
    // status change restamps the row, and a removal takes what sat under it.
    #[test]
    fn fold_keeps_the_tree_current() {
        let mut tree = sample();
        let mut renamed = agent("a1", "main", "Renamed");
        renamed.status = AgentStatus::Finished;
        fold(
            &mut tree,
            &ServerEvent::EntityUpserted {
                entity: Entity::Agent(renamed),
            },
        );
        assert_eq!(tree.agents.len(), 2, "an upsert of a known id replaces it");
        assert_eq!(tree.agents[0].name, "Renamed");
        fold(
            &mut tree,
            &ServerEvent::StatusChanged {
                agent: AgentId("a1".into()),
                status: AgentStatus::NeedsFeedback,
                changed_at: 7,
                unseen: false,
            },
        );
        assert_eq!(tree.agents[0].status, AgentStatus::NeedsFeedback);
        assert_eq!(tree.agents[0].status_changed_at, 7);
        fold(
            &mut tree,
            &ServerEvent::EntityRemoved {
                id: EntityId::Project(ProjectId("p".into())),
            },
        );
        assert!(
            tree.worktrees.is_empty() && tree.agents.is_empty(),
            "a removed project takes its rows with it: {tree:?}"
        );
    }
}
