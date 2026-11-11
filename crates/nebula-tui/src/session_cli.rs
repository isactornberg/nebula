//! One-shot clients that drive the tree from any shell: `nebula session`,
//! which starts a session in it and follows one from there. Each is
//! composed from requests the daemon
//! already answers (`Subscribe` for the Snapshot, then what the TUI sends
//! too: `CreateAgent`, `TailOutput`, `Input`, `RestartAgent`,
//! `DeleteAgent`), so a script gets sessions that sit on the grid like any
//! other. None of them starts a daemon: with none running there is no tree
//! to read and nothing to put a session in.
//!
//! This module is what they share beyond the connection and the tree it
//! keeps, which are [`nebula_client::Client`]'s.

use anyhow::{anyhow, Result};
use nebula_core::Agent;

use crate::app::Tree;

mod session;
mod start;

pub use session::{run, SessionOp};
pub(crate) use start::warn_if_statusless;
pub use start::StartOpts;

pub(crate) use nebula_client::{Client, Closed};

/// The exit of `session send` and `orchestrator send` when the text was
/// typed and the session did not report taking it, so nothing says it
/// arrived whole. Not 2, which clap exits with for a usage error, where
/// nothing was typed.
pub(crate) const EXIT_UNCONFIRMED: u8 = 3;

/// Why neither types through a daemon older than
/// [`nebula_core::TURN_RECEIPT_PROTOCOL`].
pub(crate) const CANNOT_CONFIRM: &str = "the running daemon cannot tell whether a prompt arrived \
     whole: `nebula reload` moves it onto this build, its sessions kept";

/// Why neither types into a session with a dialog open.
pub(crate) const DIALOG_OPEN: &str =
    "has a dialog open, which typed text and its Enter would answer blindly: answer it in the TUI";

/// The session with this id, as `nebula sessions` prints it.
pub(crate) fn session_by_id<'a>(tree: &'a Tree, id: &str) -> Result<&'a Agent> {
    tree.agents
        .iter()
        .find(|a| a.id.as_str() == id)
        .ok_or_else(|| no_session(id))
}

pub(crate) fn no_session(id: &str) -> anyhow::Error {
    anyhow!("no session {id}: `nebula sessions` lists them")
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
            removed: false,
        }
    }

    pub(super) fn project(path: &Path) -> Project {
        Project {
            id: ProjectId("p".into()),
            name: "orbit".into(),
            repo_path: path.to_path_buf(),
            sort_order: 0,
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
            role: nebula_core::AgentRole::Worker,
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
    fn a_session_is_found_by_its_id_or_the_error_names_nebula_sessions() {
        let tree = sample();
        assert_eq!(session_by_id(&tree, "a1").unwrap().name, "Fix Login");
        let err = session_by_id(&tree, "nope").unwrap_err().to_string();
        assert!(
            err.contains("nope") && err.contains("`nebula sessions`"),
            "{err}"
        );
    }
}
