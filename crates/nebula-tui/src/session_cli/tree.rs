//! `nebula tree`: the tree the daemon holds, printed nested for a person
//! and as one JSON object for a script.

use std::io::Write;

use anyhow::{Context, Result};
use nebula_core::{Agent, AgentKind, Project, Worktree};

use super::{printable, Client};
use crate::app::Tree;

/// CLI: `nebula tree [--json]`: print the tree the daemon holds, from the
/// one Snapshot a `Subscribe` is answered with.
pub async fn print_tree(json: bool) -> Result<()> {
    let client = Client::connect("there is no tree to print").await?;
    let mut text = if json {
        serde_json::to_string_pretty(&tree_json(&client.tree))?
    } else {
        tree_text(&client.tree)
    };
    text.push('\n');
    std::io::stdout()
        .lock()
        .write_all(text.as_bytes())
        .context("writing to stdout")
}

/// The harness a row runs, as `nebula config harnesses` names it: a custom
/// harness by its registry id, a built-in by its kind.
fn harness_id(agent: &Agent) -> &str {
    match agent.kind {
        AgentKind::Custom => agent.custom_harness.as_deref().unwrap_or("custom"),
        kind => kind.as_str(),
    }
}

/// The tree nested the way the grid nests it, a line per project, worktree
/// and session. A session's line leads with its id (what every command
/// that takes one wants), then its status, harness, model and name.
/// Archived sessions are left to `--json`.
fn tree_text(tree: &Tree) -> String {
    let mut lines = Vec::new();
    for project in &tree.projects {
        lines.push(format!(
            "{}  {}",
            printable(&project.name),
            project.repo_path.display()
        ));
        for worktree in tree.worktrees.iter().filter(|w| w.project_id == project.id) {
            let root = if worktree.is_main { "  (root)" } else { "" };
            lines.push(format!(
                "  {}  {}{root}",
                worktree.branch,
                worktree.path.display()
            ));
            for agent in &tree.agents {
                if agent.worktree_id != worktree.id || agent.archived {
                    continue;
                }
                let model = agent
                    .model
                    .as_deref()
                    .map(|m| format!(" {m}"))
                    .unwrap_or_default();
                lines.push(format!(
                    "    {}  {}  {}{model}  {}",
                    agent.id,
                    agent.status.as_str(),
                    harness_id(agent),
                    printable(&agent.name)
                ));
            }
        }
    }
    if lines.is_empty() {
        return "no projects yet: `nebula add <dir>` registers one".into();
    }
    lines.join("\n")
}

/// The same tree as one JSON object, archived sessions included. A
/// session's `session_id` is its CLI's own: the id a resume passes back.
fn tree_json(tree: &Tree) -> serde_json::Value {
    use serde_json::json;
    let session = |a: &Agent| {
        json!({
            "id": a.id,
            "name": a.name,
            "kind": harness_id(a),
            "model": a.model,
            "effort": a.effort,
            "status": a.status.as_str(),
            "alive": a.alive,
            "unseen": a.unseen,
            "archived": a.archived,
            "session_id": a.session_id,
        })
    };
    let worktree = |w: &Worktree| {
        let sessions = tree.agents.iter().filter(|a| a.worktree_id == w.id);
        json!({
            "id": w.id,
            "branch": w.branch,
            "path": w.path.to_string_lossy(),
            "root": w.is_main,
            "sessions": sessions.map(session).collect::<Vec<_>>(),
        })
    };
    let project = |p: &Project| {
        let worktrees = tree.worktrees.iter().filter(|w| w.project_id == p.id);
        json!({
            "id": p.id,
            "name": p.name,
            "path": p.repo_path.to_string_lossy(),
            "worktrees": worktrees.map(worktree).collect::<Vec<_>>(),
        })
    };
    json!({"projects": tree.projects.iter().map(project).collect::<Vec<_>>()})
}

#[cfg(test)]
mod tests {
    use super::super::tests::{agent, sample};
    use super::*;

    // The human page: nested, one line each, the id first on a session's
    // line, an empty checkout still listed and an archived session left out.
    #[test]
    fn tree_text_nests_projects_worktrees_and_sessions() {
        assert_eq!(
            tree_text(&sample()),
            "orbit  /code/orbit\n  \
               main  /code/orbit  (root)\n    \
                 a1  running  claude opus  Fix Login\n  \
               feat  /code/orbit-worktrees/feat"
        );
        assert!(tree_text(&Tree::default()).contains("nebula add"));
    }

    // A name is printed as text, whatever it was renamed to.
    #[test]
    fn tree_text_replaces_control_characters_in_a_name() {
        let mut tree = sample();
        tree.agents[0].name = "Fix\x1b[2J\nLogin".into();
        let text = tree_text(&tree);
        assert!(text.contains("Fix?[2J?Login"), "{text}");
        assert_eq!(text.lines().count(), 4, "still one line a row: {text}");
    }

    // The shape a script reads: every key the help promises, archived
    // sessions kept, and an empty checkout carrying an empty list.
    #[test]
    fn tree_json_carries_every_row_under_its_parent() {
        let json = tree_json(&sample());
        let project = &json["projects"][0];
        assert_eq!(project["name"], "orbit");
        assert_eq!(project["path"], "/code/orbit");
        let main = &project["worktrees"][0];
        assert_eq!(main["root"], true);
        assert_eq!(
            main["sessions"][0],
            serde_json::json!({
                "id": "a1",
                "name": "Fix Login",
                "kind": "claude",
                "model": "opus",
                "effort": "high",
                "status": "running",
                "alive": true,
                "unseen": false,
                "archived": false,
                "session_id": "s1",
            })
        );
        assert_eq!(main["sessions"][1]["archived"], true);
        let feat = &project["worktrees"][1];
        assert_eq!(feat["branch"], "feat");
        assert_eq!(feat["root"], false);
        assert_eq!(feat["sessions"], serde_json::json!([]));
    }

    // A custom harness is named by its registry id, never the bare kind.
    #[test]
    fn a_custom_harness_is_named_by_its_registry_id() {
        let mut row = agent("a1", "main", "x");
        row.kind = AgentKind::Custom;
        row.custom_harness = Some("agy".into());
        assert_eq!(harness_id(&row), "agy");
    }
}
