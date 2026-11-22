//! `nebula session start`: the session a script starts from any shell, in
//! the checkout a directory is in.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use nebula_core::{AgentKind, ClientRequest, EntityId, Worktree};

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

/// What `nebula session start` was asked for: the task, and each flag
/// under its own name, `dir` being `--in`.
#[derive(Debug, Default)]
pub struct StartOpts {
    pub task: String,
    pub dir: Option<String>,
    pub name: Option<String>,
    pub kind: Option<AgentKind>,
    /// The registry id `--kind` named, when `kind` is [`AgentKind::Custom`].
    pub custom_harness: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub json: bool,
}

/// What a `nebula session start` launches: the harness, as the kind plus
/// its registry id for a custom one, and the model and effort it passes.
#[derive(Debug, PartialEq)]
struct Launch {
    kind: AgentKind,
    custom_harness: Option<String>,
    model: Option<String>,
    effort: Option<String>,
}

/// The harness, model and effort `opts` launches: what it names, and for
/// each it does not, what the QUICK PROMPT launches. The effort is fitted
/// to the model last, as every launch surface does for a harness that
/// joins the two into one id.
fn launch(opts: &StartOpts, cfg: &Config) -> Launch {
    let (kind, custom_harness) = match opts.kind {
        Some(kind) => (kind, opts.custom_harness.clone()),
        None => (cfg.quick_prompt_kind(), None),
    };
    let harness = cfg.effective_harness(kind, custom_harness.as_deref());
    // A custom harness's defaults are its registry entry's; the kind-only
    // helpers know the built-ins alone.
    let (default_model, default_effort) = match kind {
        AgentKind::Custom => (
            harness.default_model().map(str::to_string),
            harness.default_effort().map(str::to_string),
        ),
        _ => (cfg.default_model(kind), cfg.default_effort(kind)),
    };
    let model = opts.model.clone().or(default_model);
    let effort = fit_effort_in(
        &harness,
        model.as_deref(),
        opts.effort.clone().or(default_effort),
    );
    Launch {
        kind,
        custom_harness,
        model,
        effort,
    }
}

/// Say on stderr, for a session just started on `kind`, when its harness
/// reports no status to nebula. The daemon stamps a session started on a
/// task `running`, and only a hook moves it on from there, so a wait on it
/// never ends by itself.
pub(crate) fn warn_if_statusless(cfg: &Config, kind: AgentKind, custom: Option<&str>) {
    let harness = cfg.effective_harness(kind, custom);
    if harness.hook_dialect().is_none() {
        eprintln!(
            "warning: the {} harness reports no status to nebula, so this session stays \
             `running`: `nebula session wait` needs --timeout, and `nebula session send` will \
             refuse it.",
            harness.id
        );
    }
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
    let Launch {
        kind,
        custom_harness,
        model,
        effort,
    } = launch(&opts, &cfg);
    let harness = cfg.effective_harness(kind, custom_harness.as_deref()).id;
    let (name, auto_title) = match opts.name.as_deref().map(str::trim) {
        Some(name) if !name.is_empty() => (name.to_string(), false),
        _ => (client.tree.free_agent_name(&home.id), true),
    };
    let created = client
        .request(|req_id| ClientRequest::CreateAgent {
            req_id,
            worktree: home.id.clone(),
            name: name.clone(),
            kind,
            custom_harness: custom_harness.clone(),
            model,
            effort,
            auto_title,
            cloud_prompt: None,
            starting_prompt: Some(task.to_string()),
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        })
        .await?;
    let Some(EntityId::Agent(id)) = created else {
        bail!("the daemon acknowledged the session without naming it");
    };
    warn_if_statusless(&cfg, kind, custom_harness.as_deref());
    let mut text = if opts.json {
        ipc::started_json(&id, &name, &home)?
    } else {
        format!(
            "started {} session {id} \"{name}\" in {}; it is working on that task now and shows \
             in the sessions list.",
            harness,
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
    use super::super::tests::worktree;
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

    // Left out, the harness is the QUICK PROMPT's with that harness's own
    // defaults; a flag names another, and its defaults follow the harness.
    #[test]
    fn launch_defaults_to_what_the_quick_prompt_launches() {
        let cfg = Config {
            quick_prompt_kind: "codex".into(),
            ..Config::default()
        };
        let quick = launch(&StartOpts::default(), &cfg);
        assert_eq!(quick.kind, AgentKind::Codex);
        assert_eq!(quick.custom_harness, None);
        assert_eq!(quick.model, cfg.default_model(AgentKind::Codex));
        assert_eq!(quick.effort, cfg.default_effort(AgentKind::Codex));

        let named = StartOpts {
            kind: Some(AgentKind::Claude),
            model: Some("sonnet".into()),
            ..StartOpts::default()
        };
        let claude = launch(&named, &cfg);
        assert_eq!(claude.kind, AgentKind::Claude);
        assert_eq!(claude.model.as_deref(), Some("sonnet"));
        assert_eq!(claude.effort, cfg.default_effort(AgentKind::Claude));

        let named = StartOpts {
            effort: Some("low".into()),
            ..StartOpts::default()
        };
        assert_eq!(launch(&named, &cfg).effort.as_deref(), Some("low"));
    }

    // A harness the config adds launches under its registry id, with the
    // model and effort its entry sets as defaults.
    #[test]
    fn launch_carries_a_registry_harness_and_its_defaults() {
        let entry: nebula_core::harness::HarnessOverride = serde_json::from_str(
            r#"{"program": "agy", "model_default": "gemini-4-neon", "effort_flag": "--effort",
                "effort_default": "high", "effort_offered": true}"#,
        )
        .unwrap();
        let cfg = Config {
            harnesses: [("agy".to_string(), entry)].into(),
            ..Config::default()
        };
        let named = StartOpts {
            kind: Some(AgentKind::Custom),
            custom_harness: Some("agy".into()),
            ..StartOpts::default()
        };
        assert_eq!(
            launch(&named, &cfg),
            Launch {
                kind: AgentKind::Custom,
                custom_harness: Some("agy".into()),
                model: Some("gemini-4-neon".into()),
                effort: Some("high".into()),
            }
        );
    }
}
