//! `nebula session start`: the session a script starts from any shell, in
//! the checkout a directory is in.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use nebula_core::{AgentKind, ClientRequest, EntityId, Worktree, WorktreeId};

use super::Client;
use crate::app::Tree;
use crate::config::{fit_effort_in, Config};
use crate::ipc;

/// How long `session start` waits for a checkout to become a row. WORKTREE
/// SYNC adopts what `git worktree add` made on its 2 s beat, so this is two
/// beats and a margin.
const ROW_WAIT: Duration = Duration::from_secs(5);

fn canonical_or_raw(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The row of the checkout rooted at `root` (already canonical). A row's
/// own path may be spelled through a symlink (macOS's `/tmp`).
fn row_at(tree: &Tree, root: &Path) -> Option<Worktree> {
    tree.worktrees
        .iter()
        .find(|w| canonical_or_raw(&w.path) == root)
        .cloned()
}

/// The main checkout of the repository the linked worktree at `root`
/// belongs to. A linked worktree's `.git` is a file naming its own git
/// directory, `<main>/.git/worktrees/<name>`, whose `commondir` leads back
/// to `<main>/.git` (the hops the daemon's sync probe follows). `None` for
/// a main checkout, whose `.git` is a directory, and for a submodule,
/// whose git directory has no `commondir`.
fn main_checkout(root: &Path) -> Option<PathBuf> {
    let pointer = std::fs::read_to_string(root.join(".git")).ok()?;
    // Git writes both pointers absolute or relative to the file they are
    // in; joining an absolute path replaces the base.
    let git_dir = root.join(pointer.trim().strip_prefix("gitdir:")?.trim());
    let common = std::fs::read_to_string(git_dir.join("commondir")).ok()?;
    let common = std::fs::canonicalize(git_dir.join(common.trim())).ok()?;
    Some(common.parent()?.to_path_buf())
}

/// The worktree whose checkout `dir` (already canonical) is in: the row at
/// the nearest `.git` above it, and never the row of a checkout around
/// that one. A repository nested in a known checkout (a vendored clone, a
/// submodule) is its own place, and nebula does not know it.
///
/// A checkout `git worktree add` made a moment ago is no row until
/// WORKTREE SYNC adopts it, so a linked worktree of a registered project is
/// waited for. Nothing else can turn up, and fails at once.
async fn worktree_of_dir(client: &mut Client, dir: &Path) -> Result<Worktree> {
    let Some(root) = dir.ancestors().find(|a| a.join(".git").exists()) else {
        bail!(
            "{} is in no git repository: `nebula add <dir>` registers one as a project",
            dir.display()
        );
    };
    if let Some(row) = row_at(&client.tree, root) {
        return Ok(row);
    }
    let main = main_checkout(root);
    let registered = main.as_deref().is_some_and(|main| {
        let projects = &client.tree.projects;
        projects
            .iter()
            .any(|p| canonical_or_raw(&p.repo_path) == main)
    });
    if !registered {
        bail!(
            "{} is in no checkout nebula knows; register its repository first: `nebula add {}`",
            dir.display(),
            main.as_deref().unwrap_or(root).display()
        );
    }
    client
        .wait_for(Some(ROW_WAIT), |tree| row_at(tree, root))
        .await?
        .with_context(|| {
            format!(
                "{} is a worktree of a project nebula knows, but the daemon has not adopted it \
                 after {} s: try again",
                root.display(),
                ROW_WAIT.as_secs()
            )
        })
}

/// The first free `agent-N` among the names in `worktree`: the default
/// name a sibling takes, and what makes the row eligible for AUTO-TITLE.
fn free_agent_name(tree: &Tree, worktree: &WorktreeId) -> String {
    (1..)
        .map(|n| format!("agent-{n}"))
        .find(|name| {
            !tree
                .agents
                .iter()
                .any(|a| &a.worktree_id == worktree && &a.name == name)
        })
        .expect("an unbounded counter always finds a free name")
}

/// What `nebula session start` was asked for: the task, and each flag
/// under its own name, `dir` being `--in`.
#[derive(Debug, Default)]
pub struct StartOpts {
    pub task: String,
    pub dir: Option<String>,
    pub name: Option<String>,
    pub kind: Option<AgentKind>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub json: bool,
}

/// The harness, model and effort `opts` launches: what it names, and for
/// each it does not, what the QUICK PROMPT launches. The effort is fitted
/// to the model last, as every launch surface does for a harness that
/// joins the two into one id.
fn launch(opts: &StartOpts, cfg: &Config) -> (AgentKind, Option<String>, Option<String>) {
    let kind = opts.kind.unwrap_or_else(|| cfg.quick_prompt_kind());
    let model = opts.model.clone().or_else(|| cfg.default_model(kind));
    let effort = fit_effort_in(
        &cfg.effective_harness(kind, None),
        model.as_deref(),
        opts.effort.clone().or_else(|| cfg.default_effort(kind)),
    );
    (kind, model, effort)
}

/// CLI: `nebula session start <task>`: start a session on the task in the
/// worktree `--in <dir>` is in (the current directory without it). The
/// task is the new CLI's first prompt, as a QUICK PROMPT's is. It behaves
/// the same inside a session as outside one: it reads no `NEBULA_AGENT_ID`.
///
/// What this prints is for whoever started the session to keep hold of it
/// by: its id, its name and its checkout, as a sentence, or as one JSON
/// object under `--json`.
pub(super) async fn start(opts: StartOpts) -> Result<()> {
    let task = opts.task.trim();
    if task.is_empty() {
        bail!(
            "the task is empty: `nebula session start <task>` needs the work the session starts on"
        );
    }
    let dir = ipc::existing_dir(opts.dir.as_deref().unwrap_or("."))?;
    let mut client = Client::connect("no session started").await?;
    let home = worktree_of_dir(&mut client, &dir).await?;
    let cfg = Config::load();
    let (kind, model, effort) = launch(&opts, &cfg);
    let (name, auto_title) = match opts.name.as_deref().map(str::trim) {
        Some(name) if !name.is_empty() => (name.to_string(), false),
        _ => (free_agent_name(&client.tree, &home.id), true),
    };
    let created = client
        .request(|req_id| ClientRequest::CreateAgent {
            req_id,
            worktree: home.id.clone(),
            name: name.clone(),
            kind,
            custom_harness: None,
            model,
            effort,
            auto_title,
            cloud_prompt: None,
            starting_prompt: Some(task.to_string()),
            issue_url: None,
        })
        .await?;
    let Some(EntityId::Agent(id)) = created else {
        bail!("the daemon acknowledged the session without naming it");
    };
    // The daemon stamps a session started on a task `running`, and only a
    // hook moves it on from there.
    if cfg.effective_harness(kind, None).hook_dialect().is_none() {
        eprintln!(
            "warning: the {} harness reports no status to nebula, so this session stays \
             `running`: `nebula session wait` needs --timeout, and `nebula session send` will \
             refuse it.",
            kind.as_str()
        );
    }
    let mut text = if opts.json {
        serde_json::to_string_pretty(&serde_json::json!({
            "id": id,
            "name": name,
            "worktree": home.id,
            "path": home.path.to_string_lossy(),
        }))?
    } else {
        format!(
            "started {} session {id} \"{name}\" in {}; it is working on that task now and shows \
             in the sessions list.",
            kind.as_str(),
            home.path.display()
        )
    };
    text.push('\n');
    std::io::stdout()
        .lock()
        .write_all(text.as_bytes())
        .context("writing to stdout")
}

#[cfg(test)]
mod tests {
    use super::super::tests::{agent, worktree};
    use super::*;

    // A row is found by its checkout's root alone, however the row spells
    // it: a directory under a checkout is not that checkout's root.
    #[test]
    fn a_row_is_found_by_the_root_of_its_checkout_through_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let real = std::fs::canonicalize(tmp.path()).unwrap();
        let root = real.join("repo");
        std::fs::create_dir_all(root.join("vendor/other")).unwrap();
        let alias = real.join("alias");
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        let tree = Tree {
            // Spelled through the symlink, as a daemon handed `/tmp/…`
            // would have stored it.
            worktrees: vec![worktree("main", &alias, true)],
            ..Tree::default()
        };
        assert_eq!(row_at(&tree, &root).unwrap().branch, "main");
        assert!(row_at(&tree, &root.join("vendor/other")).is_none());
    }

    // The two hops from a linked worktree back to its main checkout, with
    // the relative `commondir` git writes. A main checkout and a submodule
    // have no such way back.
    #[test]
    fn a_linked_worktree_leads_back_to_its_main_checkout() {
        let tmp = tempfile::tempdir().unwrap();
        let real = std::fs::canonicalize(tmp.path()).unwrap();
        let main = real.join("repo");
        let git_dir = main.join(".git/worktrees/feat");
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::write(git_dir.join("commondir"), "../..\n").unwrap();
        let linked = real.join("feat");
        std::fs::create_dir_all(&linked).unwrap();
        std::fs::write(
            linked.join(".git"),
            format!("gitdir: {}\n", git_dir.display()),
        )
        .unwrap();
        assert_eq!(main_checkout(&linked), Some(main.clone()));
        assert_eq!(main_checkout(&main), None, "its `.git` is a directory");

        let module = main.join(".git/modules/dep");
        std::fs::create_dir_all(&module).unwrap();
        let submodule = main.join("dep");
        std::fs::create_dir_all(&submodule).unwrap();
        std::fs::write(submodule.join(".git"), "gitdir: ../.git/modules/dep\n").unwrap();
        assert_eq!(main_checkout(&submodule), None);
    }

    #[test]
    fn free_agent_name_is_the_first_unused_in_that_worktree() {
        let tree = Tree {
            agents: vec![
                agent("a1", "main", "agent-1"),
                agent("a2", "main", "Fix Login"),
                agent("a3", "main", "agent-3"),
                // A row in another worktree takes no name in this one.
                agent("a4", "feat", "agent-2"),
            ],
            ..Tree::default()
        };
        assert_eq!(
            free_agent_name(&tree, &WorktreeId("main".into())),
            "agent-2"
        );
        assert_eq!(
            free_agent_name(&tree, &WorktreeId("feat".into())),
            "agent-1"
        );
    }

    // Left out, the harness is the QUICK PROMPT's with that harness's own
    // defaults; a flag names another, and its defaults follow the harness.
    #[test]
    fn launch_defaults_to_what_the_quick_prompt_launches() {
        let cfg = Config {
            quick_prompt_kind: "codex".into(),
            ..Config::default()
        };
        let (kind, model, effort) = launch(&StartOpts::default(), &cfg);
        assert_eq!(kind, AgentKind::Codex);
        assert_eq!(model, cfg.default_model(AgentKind::Codex));
        assert_eq!(effort, cfg.default_effort(AgentKind::Codex));

        let named = StartOpts {
            kind: Some(AgentKind::Claude),
            model: Some("sonnet".into()),
            ..StartOpts::default()
        };
        let (kind, model, effort) = launch(&named, &cfg);
        assert_eq!(kind, AgentKind::Claude);
        assert_eq!(model.as_deref(), Some("sonnet"));
        assert_eq!(effort, cfg.default_effort(AgentKind::Claude));

        let named = StartOpts {
            effort: Some("low".into()),
            ..StartOpts::default()
        };
        assert_eq!(launch(&named, &cfg).2.as_deref(), Some("low"));
    }
}
