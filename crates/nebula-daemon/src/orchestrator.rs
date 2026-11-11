//! Privileged commands for the ORCHESTRATOR SESSION.
//!
//! The CLI wrappers are just another same-user socket client, so prompt text
//! is not part of the trust boundary. Every operation here starts by checking
//! the caller row persisted in SQLite is the orchestrator.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use nebula_core::{
    Agent, AgentId, AgentKind, AgentRole, AgentStatus, EntityId, Link, OutputTail, Project,
    ProjectId, ServerEvent, SessionRef, TerminalTab, Worktree, WorktreeId, MAX_CLOUD_PROMPT_BYTES,
};

use crate::registry::{CreateAgentSpec, Daemon, TRUST_WAIT};

pub const CLAUDE_ORCHESTRATOR_GUIDANCE: &str = "[nebula] You are the pinned ORCHESTRATOR \
session for nebula. Your job is to coordinate work across every project, worktree and session \
without making the user click through them. You have these orchestrator-only commands; the daemon \
will refuse them unless this session is the persisted orchestrator:\n\n  nebula orchestrator list \
[--json]\n  nebula orchestrator captain <project-id-or-name> [--json]\n  nebula orchestrator read \
<session-id> [--bytes N | --report]\n  nebula orchestrator send <session-id> \"<prompt>\"\n  nebula \
orchestrator spawn --project <project-id-or-name> [--worktree <branch-or-id> [--base <ref>]] \
[--kind <harness>] [--name <title>] [--model <m>] [--effort <e>] [--json] \"<task>\"\n  nebula \
orchestrator review <session-id>\n\nThe user talks to you full-screen in nebula's orchestrator \
view, beside a board of every session that needs them. You run in nebula's own orchestrator folder, \
not in any project: act on projects through these commands and the sessions in them. Use `list \
--json` before acting when you need current IDs or status. `read` prints a Claude session's recent \
conversation from its transcript, and any other session's screen, where text in its input box is a \
suggestion the CLI offers, not a prompt anyone sent. `read --report` prints only a Claude \
session's last reply, every line of it: once its turn is done, that is its report.\n\nProject work goes to that project's CAPTAIN, one long-lived session \
per project that starts and follows its own workers. Get it by running `captain <project>` as a \
background command: it starts one when the project has none and prints its id once it is up. A \
new captain may first show Claude's question whether to trust the project's folder, which only the \
user answers, and `captain` waits for that, up to 10 minutes. So right after starting it, run \
`sleep 5; nebula orchestrator list --json`: when that captain is `needs_feedback`, tell the user \
plainly \"Claude asks whether to trust <the project's folder>; F9 brings it up\" and nothing more \
about it. When `captain` returns, `send` the captain the request in the user's words, naming by id \
the sessions it is about. Then run `nebula session wait <captain-id> --until-done` as \
a background command: it returns once the captain is done, its workers' results read, and the \
captain's report holds a line `DONE …`, `BLOCKED …` or `QUESTION …` near its end; `read \
<captain-id> --report` prints that report on its own. The user may also talk to a captain \
directly: work a captain reports that \
you did not ask for is the user's own, so report it as theirs and never disown it. You \
never change a project's files, or build or run anything in one, yourself, however small the \
change: when `captain` or `send` fails, tell the user why instead of doing the work another way. \
Answer a quick question about where things stand yourself, from `list --json`, without a captain hop. For \
work across several projects, send each captain its part; when one part depends on another, send \
the later part only once the earlier captain reports DONE. A captain's reply that only reports \
status needs no comment from you. Use `spawn` only when the user asks for a session of their own; \
it prints the new session's id, and `--json` prints it with the session's name, worktree and path. \
Add `--kind`, `--model` or `--effort` only when the user names one, `--name` to give the session a \
title it keeps, and `--base` only for a new worktree's start point the user names. After spawning \
work the user will want to hear about, wait on it the same way, and when the wait returns, read \
what the session did with `read <id> --report` and report it. Wait only when there is a next step to take on the result; \
never block your turn on a wait.\n\n`send` returns once the session has taken the whole prompt \
(exit 3: typed, but not taken within 5 s, so `read` it before sending again), fails when the \
session took only part of it and works on that, so send the whole prompt again saying the last one \
was cut off, and refuses a session with a dialog open. When no \
wait of yours tells you the session is done with it, nebula types in a note once it is, `[nebula] … \
is done with what you sent it. …`: read what it did then with `read <id> --report` and report it. A wait \
goes on through a session's dialogs, so whenever you report while the user may be away, also check \
`nebula orchestrator list --json` for sessions waiting on the user (`needs_feedback`), a captain's \
workers included, and name them: they stay stuck until the user answers. Use `review` when the \
user wants to see a session: nebula brings it up full-screen in place of this chat, and ^q brings \
them back to you. It only helps when the user is at the nebula TUI. Keep your replies short: the \
user may be reading them on a phone. Default to tell-and-ask for permission prompts: do not approve \
another agent's permission request unless the user explicitly tells you to. Never answer a dialog \
for the user or offer to, and never ask the user to run a `nebula` command: they are yours to run, \
and the user answers a dialog in its pane, which F9 brings up.";

pub const CLAUDE_CAPTAIN_GUIDANCE: &str = "[nebula] You are this project's CAPTAIN: the one \
long-lived session that owns all work in this one project, the repository this session runs in. \
nebula's ORCHESTRATOR routes the user's requests for this project to you, typed in as your next \
turn; the user may also talk to you directly. These rules come before nebula's other guidance.\n\n\
Answer questions about the code yourself, by reading this checkout. You cannot edit files here, and \
you never change, build or run anything yourself. Your Bash is for reading (`git log`, `git diff`, \
`gh pr view` and the like, a worker's work by its branch from this checkout, as in `git show \
<branch>:<file>`, never with `git -C`) and for `nebula` commands only: nebula denies anything else without \
asking, so never look for another way round a denial. Anything that needs edits, builds or running \
code becomes a WORKER, started with\n\n  nebula spawn --worktree <branch> --name <short title> --json \
\"$(cat <<'EOF'\n<task>\nEOF\n)\"\n\nwith the task in that quoted heredoc, never in quotes, a fresh \
branch for each independent piece of work and a title of a few words that \
names it (`Login redirect`). In your reports, name each worker by its title, never by its id. \
Never run `nebula worktree`: you stay in this \
checkout. Make the task self-contained, pass the user's words on exactly, and never widen it. End \
every task with: \"End your final reply with one line `DONE <what you did>`, `BLOCKED <why>` or \
`QUESTION <what you need>`, and list anything a future agent in this repo should know as `Notes:` \
lines above it.\" Run at most 4 workers at once. A follow-up to work a worker already did goes to \
that worker, not a new one: run `nebula ask <id> --no-wait --timeout 3600` with the follow-up after \
it in a quoted heredoc like a task's, as a background command, and it prints the worker's reply \
when that turn ends.\n\nAfter starting a \
worker, run `nebula session wait <id> --until-done` as a background command; never block your turn \
on it. When it returns, read the worker's report with `nebula read <id> --report`, decide the next step in \
this project, and report. The wait goes on through a worker's dialogs, which only the user answers: \
when you report while one is open, say which worker waits on the user.\n\nWhen a pull request from \
this project merges, nebula types in a note: `[nebula] PR #<n> \"<title>\" merged from <branch>. \
Sessions that worked on it: <ids>.` Read each of those sessions' final replies with `nebula read \
<id> --report`, skipping one it no longer finds, collect their `Notes:` lines, and save what a future agent \
in this repository would otherwise get wrong into your Claude memory for this repo: keep it short, \
merge duplicates and drop what is stale. Your memory is the one place you can write. When a doc in \
the repository is now out of date, start a worker to fix it in a pull request. When there is a \
decision worth recording in the user's notes, put one line `To record: <the decision>` just above \
your last line and write nothing there yourself. The reply only reports status: end it `DONE \
<what you saved>`.\n\nEnd every reply with one \
line `DONE <what>`, `BLOCKED <why>` or `QUESTION <what>`: nebula's board and the orchestrator read \
it. `QUESTION` is only for an answer you need from the user to go on, and `BLOCKED` only for work \
that cannot go on; a reply that says what you started ends `DONE started <what>`. Keep replies short. Never merge a pull request, never close or delete sessions or worktrees, and \
have a pull request opened only when the user asks, by the worker that did the work. Your commands reach only this project: when a request \
needs another one, say so, and the orchestrator routes it.";

/// A spawn's system-prompt rule (`pr_scope::combined_rule`), with the
/// guidance of the ORCHESTRATOR or a CAPTAIN ahead of it for that role.
pub(crate) fn launch_rule(role: AgentRole, rule: Option<String>) -> Option<String> {
    let guidance = match role {
        AgentRole::Worker => return rule,
        AgentRole::Orchestrator => CLAUDE_ORCHESTRATOR_GUIDANCE,
        AgentRole::Captain => CLAUDE_CAPTAIN_GUIDANCE,
    };
    Some(match rule {
        Some(rule) => format!("{guidance}\n\n{rule}"),
        None => guidance.to_string(),
    })
}

/// What a CAPTAIN's Claude runs unasked under `--permission-mode
/// dontAsk`, which denies everything else unasked, whatever the user's own
/// permission mode: the file tools that only read, the `nebula` commands
/// that start, follow and read its workers, and git and gh that only read.
/// Claude lets commands it knows only read (`ls`, `cat`) and a write to its
/// own memory folder through on its own.
const CAPTAIN_ALLOWED_TOOLS: &[&str] = &[
    "Read",
    "Grep",
    "Glob",
    "Bash(nebula spawn:*)",
    "Bash(nebula ask:*)",
    "Bash(nebula read:*)",
    "Bash(nebula sessions:*)",
    "Bash(nebula session wait:*)",
    "Bash(nebula session read:*)",
    "Bash(git log:*)",
    "Bash(git status:*)",
    "Bash(git diff:*)",
    "Bash(git show:*)",
    // Never `git branch:*`: `git branch -v -D <b>` deletes.
    "Bash(git branch)",
    "Bash(git branch -a)",
    "Bash(git branch -r)",
    "Bash(git branch --list:*)",
    "Bash(git branch --show-current)",
    "Bash(gh pr view:*)",
    "Bash(gh pr list:*)",
    "Bash(gh issue view:*)",
    "Bash(gh issue list:*)",
];

/// Commands a CAPTAIN never runs even where the user's own settings allow
/// them, since a deny wins over every allow: git and gh that write, and
/// `nebula worktree`, which would move it out of its checkout.
const CAPTAIN_DENIED_COMMANDS: &[&str] = &[
    "Bash(git commit:*)",
    "Bash(git push:*)",
    "Bash(git merge:*)",
    "Bash(git pull:*)",
    "Bash(git reset:*)",
    "Bash(git rebase:*)",
    "Bash(git checkout:*)",
    "Bash(git switch:*)",
    "Bash(git restore:*)",
    "Bash(git stash:*)",
    "Bash(git clean:*)",
    "Bash(gh pr merge:*)",
    "Bash(gh pr create:*)",
    "Bash(nebula worktree:*)",
];

/// What a CAPTAIN's Claude may not do even where the user's own settings
/// allow it: [`CAPTAIN_DENIED_COMMANDS`], and edit the project's root
/// checkout (`repo`), the folder its worktrees go in and the directories
/// its SPACE shares (`add_dirs`), as `Edit` rules, which hold for `Write`
/// too. Its Claude memory for the repository, which Claude keeps under its
/// own config folder, stays writable; `dontAsk` denies every other edit.
/// With no repository known, every editing tool.
fn captain_disallowed_tools(repo: Option<&Path>, add_dirs: &[std::path::PathBuf]) -> String {
    let mut tools: Vec<String> = match repo {
        None => vec!["Edit".into(), "Write".into(), "NotebookEdit".into()],
        Some(repo) => {
            let denied = [repo.to_path_buf(), crate::git::worktrees_root(repo)];
            // `//` opens an absolute path in a permission rule.
            std::iter::once("NotebookEdit".to_string())
                .chain(
                    denied
                        .iter()
                        .chain(add_dirs)
                        .map(|dir| format!("Edit(/{}/**)", dir.display())),
                )
                .collect()
        }
    };
    tools.extend(CAPTAIN_DENIED_COMMANDS.iter().map(|rule| rule.to_string()));
    tools.join(",")
}

/// What a Claude launch and resume carry ahead of the rest of their argv:
/// for the ORCHESTRATOR while the `orchestrator_remote_control` SETTING
/// (`remote_control`) is on, `--remote-control <name>`, which opens the
/// session to the Claude app under the [`remote_control_name`] of this
/// instance; for a CAPTAIN, `--permission-mode dontAsk` with only what it
/// needs allowed ([`CAPTAIN_ALLOWED_TOOLS`]) and what it may never do in
/// its project `repo` denied ([`captain_disallowed_tools`]), so it reads
/// and starts workers but changes nothing, however the user's own Claude
/// is set; and for every role, `--add-dir` for each of `add_dirs`, the
/// directories its project's SPACE shares. Nothing for any other harness.
/// Each takes its value in the same argument, as the tool and directory
/// flags take a list that would otherwise swallow the arguments after
/// them.
pub(crate) fn claude_launch_args(
    role: AgentRole,
    kind: AgentKind,
    remote_control: bool,
    repo: Option<&Path>,
    add_dirs: &[std::path::PathBuf],
) -> Vec<String> {
    if kind != AgentKind::Claude {
        return Vec::new();
    }
    let mut args = Vec::new();
    match role {
        AgentRole::Orchestrator if remote_control => {
            let data_dir = nebula_core::env::non_empty(nebula_core::env::DATA_DIR);
            args.extend([
                "--remote-control".into(),
                remote_control_name(data_dir.as_deref()),
            ]);
        }
        AgentRole::Captain => args.extend([
            "--permission-mode=dontAsk".to_string(),
            format!("--allowedTools={}", CAPTAIN_ALLOWED_TOOLS.join(",")),
            format!(
                "--disallowedTools={}",
                captain_disallowed_tools(repo, add_dirs)
            ),
        ]),
        _ => {}
    }
    args.extend(
        add_dirs
            .iter()
            .map(|dir| format!("--add-dir={}", dir.display())),
    );
    args
}

/// The name the ORCHESTRATOR goes by in the Claude app: `orchestrator` on
/// nebula's own data dir, and `orchestrator-dev-<checkout>` on one that
/// `NEBULA_DATA_DIR` moves, as it does for every `make dev` instance. The
/// checkout is the data dir's name less the slot hash `make dev` puts after
/// it, so a phone that reaches the release orchestrator and a dev one, or
/// two checkouts' dev ones, tells them apart.
fn remote_control_name(data_dir: Option<&str>) -> String {
    let Some(dir) = data_dir else {
        return "orchestrator".into();
    };
    let name = std::path::Path::new(dir)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let checkout = match name.rsplit_once('-') {
        Some((checkout, slot))
            if slot.len() == 8 && slot.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            checkout
        }
        _ => &name,
    };
    if checkout.is_empty() {
        "orchestrator-dev".into()
    } else {
        format!("orchestrator-dev-{checkout}")
    }
}

pub type OrchestratorTree = (
    Vec<Project>,
    Vec<Worktree>,
    Vec<Agent>,
    Vec<TerminalTab>,
    Vec<Link>,
);

impl Daemon {
    pub(crate) fn require_orchestrator(&self, caller: &AgentId) -> Result<Agent> {
        let agent = self
            .store
            .get_agent(caller)?
            .context("orchestrator session not found")?;
        if agent.archived {
            bail!("orchestrator session is archived");
        }
        if agent.role != AgentRole::Orchestrator {
            bail!("this command is only available to the orchestrator session");
        }
        Ok(agent)
    }

    pub fn orchestrator_tree(&self, caller: &AgentId) -> Result<OrchestratorTree> {
        self.require_orchestrator(caller)?;
        let (projects, worktrees, mut agents, mut terminals) = self.store.load_tree()?;
        for a in &mut agents {
            a.alive = self.is_alive(&SessionRef::Agent(a.id.clone()));
        }
        for t in &mut terminals {
            t.alive = self.is_alive(&SessionRef::Terminal(t.id.clone()));
        }
        Ok((
            projects,
            worktrees,
            agents,
            terminals,
            self.store.load_links()?,
        ))
    }

    /// `nebula orchestrator send`: type `message` into `target` as its next
    /// turn, booting it first when it has no live process, once its CLI is
    /// up ([`Self::await_session_up`]) and never into a question it asks
    /// before that; then whether it took the whole of it
    /// ([`Self::type_turn`]).
    pub async fn orchestrator_send(
        self: &Arc<Self>,
        caller: &AgentId,
        target: &AgentId,
        message: &str,
    ) -> Result<bool> {
        self.require_orchestrator(caller)?;
        let text = validate_message(message)?;
        let agent = self
            .store
            .get_agent(target)?
            .context("target agent not found")?;
        if agent.archived {
            bail!("target agent is archived");
        }
        if agent.cloud_session_id.is_some() {
            bail!("target agent is a Claude Cloud session; use its cloud message path");
        }
        let sref = SessionRef::Agent(target.clone());
        self.ensure_session(&sref, crate::pty::DEFAULT_COLS, crate::pty::DEFAULT_ROWS)?;
        if let Err(why) = self.await_session_up(target, Duration::ZERO).await {
            let dir = self
                .store
                .get_worktree(&agent.worktree_id)?
                .map(|w| w.path)
                .unwrap_or_default();
            bail!(
                "nothing was sent: {}",
                why.say(target, &dir, true, "send again")
            );
        }
        let sent_at = nebula_core::clock::now_ms();
        let session = self
            .session(&sref)
            .context("target agent lost its terminal before the text was typed")?;
        let confirmed = self.type_turn(target, &session, text).await?;
        self.tell_when_done(caller, target, sent_at);
        Ok(confirmed)
    }

    pub fn orchestrator_read(
        &self,
        caller: &AgentId,
        session: &SessionRef,
        max_bytes: u32,
        report: bool,
    ) -> Result<Option<OutputTail>> {
        self.require_orchestrator(caller)?;
        if report {
            return Ok(Some(OutputTail {
                cols: 0,
                rows: 0,
                end_seq: 0,
                data: self.read_report(session)?.into_bytes(),
            }));
        }
        Ok(self.read_output_tail_clean(session, max_bytes))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn orchestrator_spawn(
        self: &Arc<Self>,
        caller: &AgentId,
        project: &ProjectId,
        worktree: Option<WorktreeId>,
        branch: Option<String>,
        base: Option<String>,
        name: String,
        kind: AgentKind,
        custom_harness: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        starting_prompt: String,
    ) -> Result<EntityId> {
        self.require_orchestrator(caller)?;
        let worktree = match worktree {
            Some(id) => {
                let wt = self
                    .store
                    .get_worktree(&id)?
                    .context("worktree not found")?;
                if &wt.project_id != project {
                    bail!("worktree does not belong to the requested project");
                }
                wt
            }
            None => {
                if branch.as_ref().is_none_or(|b| b.trim().is_empty()) {
                    if let Some(existing) = self
                        .store
                        .load_tree()?
                        .1
                        .into_iter()
                        .filter(|w| &w.project_id == project)
                        .min_by_key(|w| (!w.is_main, w.sort_order))
                    {
                        existing
                    } else {
                        bail!("project has no worktrees");
                    }
                } else {
                    let branch = slugify_branch(branch.unwrap_or_default().as_str());
                    let branch = if branch.is_empty() {
                        random_branch_name()
                    } else {
                        branch
                    };
                    if let Some(existing) = self
                        .store
                        .load_tree()?
                        .1
                        .into_iter()
                        .find(|w| &w.project_id == project && w.branch == branch)
                    {
                        existing
                    } else {
                        let created = self
                            .create_worktree(project, &branch, base.as_deref())
                            .await?;
                        let EntityId::Worktree(id) = created else {
                            unreachable!("create_worktree returns a worktree id");
                        };
                        self.store
                            .get_worktree(&id)?
                            .context("created worktree not found")?
                    }
                }
            }
        };
        let (_, _, agents, _) = self.store.load_tree()?;
        let taken = agents
            .iter()
            .filter(|a| a.worktree_id == worktree.id)
            .map(|a| a.name.clone())
            .collect::<Vec<_>>();
        // A name the orchestrator gave stays, as `session start --name`'s
        // does: AUTO-TITLE takes only the default `agent-N`.
        let auto_title = name.trim().is_empty();
        let name = if auto_title {
            crate::sibling::sibling_name(&taken)
        } else {
            name
        };
        self.create_agent(CreateAgentSpec {
            worktree: worktree.id,
            name,
            kind,
            custom_harness,
            model,
            effort,
            auto_title,
            cloud_prompt: None,
            starting_prompt: Some(starting_prompt),
            pr_url: None,
            issue_url: None,
            role: AgentRole::Worker,
            fork_of: None,
        })
        .await
    }

    /// `nebula orchestrator captain <project>`: the project's CAPTAIN, a
    /// Claude session named `captain` in its root checkout, found among its
    /// sessions or, with none there, started on no prompt (the
    /// orchestrator's `send` gives it work), and either way up and ready
    /// for that ([`Self::await_captain_up`]). An archived captain is
    /// retired: the next call starts a new one. A root checkout gone from
    /// disk is refused, whether a captain is there to resume or not.
    pub async fn orchestrator_captain(
        self: &Arc<Self>,
        caller: &AgentId,
        project: &ProjectId,
    ) -> Result<EntityId> {
        self.require_orchestrator(caller)?;
        // Two calls for one project, side by side, must not start two. Once
        // the row is in the store a second call finds it, so the wait for
        // the captain to come up is outside the gate.
        let gate = self
            .captain_ops
            .lock()
            .entry(project.clone())
            .or_default()
            .clone();
        let (id, root) = {
            let _one = gate.lock().await;
            let (_, worktrees, agents, _) = self.store.load_tree()?;
            let root = worktrees
                .iter()
                .find(|w| &w.project_id == project && w.is_main)
                .context("project has no root checkout")?
                .clone();
            if !root.path.is_dir() {
                bail!(
                    "the project's root checkout {} is gone from disk; point the project at its \
                     new folder before asking for its captain",
                    root.path.display()
                );
            }
            let in_project = |a: &&Agent| {
                worktrees
                    .iter()
                    .any(|w| w.id == a.worktree_id && &w.project_id == project)
            };
            let found =
                nebula_core::live_captain(agents.iter().filter(in_project)).map(|a| a.id.clone());
            let id = match found {
                Some(id) => id,
                None => match self.start_captain(&root).await? {
                    EntityId::Agent(id) => id,
                    _ => unreachable!("create_agent returns an agent id"),
                },
            };
            (id, root)
        };
        self.await_captain_up(&id, &root.path).await?;
        Ok(EntityId::Agent(id))
    }

    /// Start a CAPTAIN in the root checkout `root`, on no prompt.
    async fn start_captain(self: &Arc<Self>, root: &Worktree) -> Result<EntityId> {
        self.create_agent(CreateAgentSpec {
            worktree: root.id.clone(),
            name: "captain".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            pr_url: None,
            issue_url: None,
            role: AgentRole::Captain,
            fork_of: None,
        })
        .await
    }

    /// Wait until the CAPTAIN `id`'s Claude is up, booting it first when it
    /// has no live process ([`Self::await_session_up`]): text sent before
    /// that would answer the question it asks first blindly. A new
    /// captain's question whether to trust its folder is waited on for
    /// [`TRUST_WAIT`]: the board lists it under needs you, and the work
    /// goes on once the user has answered it there.
    async fn await_captain_up(self: &Arc<Self>, id: &AgentId, root: &Path) -> Result<()> {
        let sref = SessionRef::Agent(id.clone());
        self.ensure_session(&sref, crate::pty::DEFAULT_COLS, crate::pty::DEFAULT_ROWS)?;
        self.await_session_up(id, TRUST_WAIT).await.map_err(|why| {
            anyhow::anyhow!(why.say(id, root, true, "run `captain` again in the background"))
        })
    }

    pub fn orchestrator_open_review(
        &self,
        caller: &AgentId,
        sessions: Vec<SessionRef>,
    ) -> Result<()> {
        self.require_orchestrator(caller)?;
        if sessions.is_empty() {
            bail!("no sessions named for review");
        }
        for session in &sessions {
            match session {
                SessionRef::Agent(id) => {
                    self.store.get_agent(id)?.context("agent not found")?;
                }
                SessionRef::Terminal(id) => {
                    self.store.get_terminal(id)?.context("terminal not found")?;
                }
            }
        }
        self.broadcast(ServerEvent::ReviewOpened {
            opener: caller.clone(),
            sessions,
            tabs: Vec::new(),
        });
        Ok(())
    }
}

/// The work the ORCHESTRATOR sent, until it is told that a session is done
/// with it: by its own `session wait --until-done` on that session, or,
/// with none, by a DONE NOTE nebula types into it.
// ponytail: in memory, so a daemon restart drops the notes still owed;
// the board still shows each such session done.
#[derive(Default)]
pub(crate) struct SentWork {
    /// Each session sent work, by its latest send: one note per session.
    sent: HashMap<AgentId, Sent>,
    /// `AwaitDone`s still waiting, per session: the one that answers tells
    /// the orchestrator, so no note goes meanwhile.
    waits: HashMap<AgentId, usize>,
    /// The orchestrator's status stamp when it was last typed a note: the
    /// next waits for that to move on, as the word that it took the note
    /// lags the keys.
    typed: Option<(AgentId, i64)>,
}

#[derive(Clone, Copy, PartialEq)]
struct Sent {
    at: i64,
    /// The session has started on it, so a wait that answers from here on
    /// answers for it, not for the turn before.
    started: bool,
}

/// An `AwaitDone` on `id`, counted in [`SentWork::waits`] while it lives.
pub(crate) struct DoneWait {
    daemon: Arc<Daemon>,
    id: AgentId,
}

impl DoneWait {
    /// The wait has answered: the orchestrator that ran it needs no note.
    pub(crate) fn answered(&self) {
        let mut work = self.daemon.sent_work.lock();
        if work.sent.get(&self.id).is_some_and(|s| s.started) {
            work.sent.remove(&self.id);
        }
    }
}

impl Drop for DoneWait {
    fn drop(&mut self) {
        let mut work = self.daemon.sent_work.lock();
        if let Some(n) = work.waits.get_mut(&self.id) {
            *n -= 1;
            if *n == 0 {
                work.waits.remove(&self.id);
            }
        }
    }
}

impl Daemon {
    pub(crate) fn done_wait(self: &Arc<Self>, id: &AgentId) -> DoneWait {
        *self.sent_work.lock().waits.entry(id.clone()).or_default() += 1;
        DoneWait {
            daemon: self.clone(),
            id: id.clone(),
        }
    }

    /// Once `target` is done with what the ORCHESTRATOR `orchestrator`
    /// sent it at `sent_at`, type the orchestrator a DONE NOTE saying so,
    /// unless a wait of its own has said so first or a later send to
    /// `target` owns the news. The note waits for the orchestrator to be at
    /// rest, with its input box empty, and to have taken the last note.
    fn tell_when_done(self: &Arc<Self>, orchestrator: &AgentId, target: &AgentId, sent_at: i64) {
        let sent = Sent {
            at: sent_at,
            started: false,
        };
        self.sent_work.lock().sent.insert(target.clone(), sent);
        let daemon = self.clone();
        let (orchestrator, target) = (orchestrator.clone(), target.clone());
        tokio::spawn(async move {
            if daemon
                .note_when_done(&orchestrator, &target, sent)
                .await
                .is_none()
            {
                let mut work = daemon.sent_work.lock();
                if work.sent.get(&target).is_some_and(|s| s.at == sent.at) {
                    work.sent.remove(&target);
                }
            }
        });
    }

    /// [`Self::tell_when_done`]'s wait: `None` when no note is to go.
    async fn note_when_done(
        self: &Arc<Self>,
        orchestrator: &AgentId,
        target: &AgentId,
        sent: Sent,
    ) -> Option<()> {
        let ours = |work: &SentWork| work.sent.get(target).is_some_and(|s| s.at == sent.at);
        // A session still at rest from before its send would read as done.
        loop {
            let id = target.clone();
            let agent = self
                .store_blocking(move |store| store.get_agent(&id))
                .await
                .ok()??;
            if agent.archived || !ours(&self.sent_work.lock()) {
                return None;
            }
            if !crate::session_context::is_idle(agent.status) || agent.status_changed_at > sent.at {
                break;
            }
            tokio::time::sleep(crate::session_context::DONE_POLL).await;
        }
        self.sent_work
            .lock()
            .sent
            .get_mut(target)
            .filter(|s| s.at == sent.at)?
            .started = true;
        self.await_done(target).await.ok()?;
        let note = {
            let id = target.clone();
            self.store_blocking(move |store| done_note(store, &id))
                .await
                .ok()?
        };
        loop {
            tokio::time::sleep(crate::session_context::DONE_POLL).await;
            {
                let work = self.sent_work.lock();
                if !ours(&work) {
                    return None;
                }
                if work.waits.contains_key(target) {
                    continue;
                }
            }
            let daemon = self.clone();
            let id = orchestrator.clone();
            let ready = tokio::task::spawn_blocking(move || daemon.ready_for_note(&id))
                .await
                .ok()?
                .ok()?;
            let Some((session, stamp)) = ready else {
                continue;
            };
            {
                let mut work = self.sent_work.lock();
                if !ours(&work) {
                    return None;
                }
                let typed = Some((orchestrator.clone(), stamp));
                if work.waits.contains_key(target) || work.typed == typed {
                    continue;
                }
                work.typed = typed;
                work.sent.remove(target);
            }
            if let Err(err) = crate::session_context::write_prompt(&session, &note).await {
                tracing::warn!(%target, "done note not typed: {err}");
            }
            return Some(());
        }
    }

    /// The ORCHESTRATOR `id`'s terminal, and its status stamp, while a note
    /// can be typed into it: live, its last turn finished and settled, left
    /// alone by the user for [`TYPING_QUIET`], and its input box empty. An
    /// error when it is no longer the orchestrator.
    fn ready_for_note(&self, id: &AgentId) -> Result<Option<(Arc<crate::pty::PtySession>, i64)>> {
        let agent = self.require_orchestrator(id)?;
        let settled = nebula_core::clock::now_ms() - agent.status_changed_at
            >= nebula_core::DONE_SETTLE.as_millis() as i64;
        if !matches!(agent.status, AgentStatus::Fresh | AgentStatus::Finished) || !settled {
            return Ok(None);
        }
        Ok(self
            .session(&SessionRef::Agent(id.clone()))
            .filter(|s| s.keyed_at().is_none_or(|at| at.elapsed() >= TYPING_QUIET))
            .filter(|s| nebula_core::dialog::box_is_empty(&s.tail(NOTE_SCREEN_BYTES, None)))
            .map(|s| (s, agent.status_changed_at)))
    }
}

/// How much of the orchestrator's screen is read for its input box: what
/// the TUI reads a screen with.
const NOTE_SCREEN_BYTES: usize = 64 * 1024;
/// How long the user must have sent the orchestrator no key for a note to
/// be typed into it, as the TUI waits to type a MERGE NOTE.
const TYPING_QUIET: Duration = Duration::from_secs(5);

/// The DONE NOTE for `id`, on one line.
fn done_note(store: &crate::store::Store, id: &AgentId) -> Result<String> {
    let agent = store.get_agent(id)?.context("session not found")?;
    let project = store
        .get_worktree(&agent.worktree_id)?
        .and_then(|w| store.get_project(&w.project_id).ok().flatten())
        .map_or_else(|| "its project".to_string(), |p| p.name);
    let who = if agent.role == AgentRole::Captain {
        format!("The captain of {project}, {id},")
    } else {
        format!("Session {id} in {project}")
    };
    // Only a Claude session has a report to read on its own.
    let report = if agent.kind == AgentKind::Claude {
        " --report"
    } else {
        ""
    };
    Ok(format!(
        "[nebula] {who} is done with what you sent it. Read what it did with nebula orchestrator \
         read {id}{report}."
    ))
}

pub(crate) fn validate_message(message: &str) -> Result<&str> {
    let text = message.trim();
    if text.is_empty() {
        bail!("message is empty");
    }
    if text.contains('\0') {
        bail!("message cannot contain NUL bytes");
    }
    if text.len() > MAX_CLOUD_PROMPT_BYTES {
        bail!(
            "message is too long (max {} KiB)",
            MAX_CLOUD_PROMPT_BYTES / 1024
        );
    }
    Ok(text)
}

fn slugify_branch(raw: &str) -> String {
    raw.trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn random_branch_name() -> String {
    format!("orchestrator-{}", nebula_core::clock::now_ms())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::HookEnv;
    use crate::store::Store;
    use nebula_core::{AgentStatus, ProjectId};

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
                name: "demo".into(),
                repo_path: "/tmp/demo".into(),
                sort_order: 0,
            })
            .unwrap();
        daemon
            .store
            .insert_worktree(&Worktree {
                id: WorktreeId("w".into()),
                project_id: ProjectId("p".into()),
                path: "/tmp/demo".into(),
                branch: "main".into(),
                is_main: true,
                sort_order: 0,
                removed: false,
            })
            .unwrap();
        daemon
    }

    fn agent(id: &str, role: AgentRole) -> Agent {
        Agent {
            id: AgentId(id.into()),
            worktree_id: WorktreeId("w".into()),
            name: id.into(),
            status: AgentStatus::Fresh,
            archived: false,
            archived_at: 0,
            unseen: false,
            status_changed_at: 0,
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            issue_url: None,
            role,
            sort_order: 0,
            alive: false,
            recent_prompts: Vec::new(),
        }
    }

    #[test]
    fn the_orchestrator_and_a_captain_launch_with_their_guidance() {
        assert_eq!(launch_rule(AgentRole::Worker, None), None);
        assert_eq!(
            launch_rule(AgentRole::Worker, Some("rule".into())).as_deref(),
            Some("rule")
        );
        assert_eq!(
            launch_rule(AgentRole::Orchestrator, None).as_deref(),
            Some(CLAUDE_ORCHESTRATOR_GUIDANCE)
        );
        let both = launch_rule(AgentRole::Orchestrator, Some("rule".into())).unwrap();
        assert!(both.starts_with(CLAUDE_ORCHESTRATOR_GUIDANCE) && both.ends_with("\n\nrule"));
        assert_eq!(
            launch_rule(AgentRole::Captain, None).as_deref(),
            Some(CLAUDE_CAPTAIN_GUIDANCE)
        );
    }

    #[test]
    fn only_the_orchestrators_claude_opens_to_remote_control_and_only_when_asked() {
        let args = |role, kind, on| claude_launch_args(role, kind, on, None, &[]);
        let on = args(AgentRole::Orchestrator, AgentKind::Claude, true);
        assert_eq!(on.len(), 2);
        assert_eq!(on[0], "--remote-control");
        assert!(args(AgentRole::Orchestrator, AgentKind::Claude, false).is_empty());
        assert!(args(AgentRole::Worker, AgentKind::Claude, true).is_empty());
        assert!(args(AgentRole::Orchestrator, AgentKind::Codex, true).is_empty());
    }

    // A captain asks nothing, runs only what it is allowed, edits nothing
    // in its project's checkouts or its space's shared directories, its
    // Claude memory aside, and writes nothing with git or gh even where the
    // user allows it; every Claude in a
    // space with shared directories can reach them; each flag carries its
    // list in one argument, so it cannot swallow the prompt after it.
    #[test]
    fn a_captain_launches_unable_to_edit_its_project_and_any_claude_with_its_spaces_dirs() {
        let dirs = [
            std::path::PathBuf::from("/notes"),
            std::path::PathBuf::from("/docs"),
        ];
        let repo = Path::new("/code/api");
        let denied = CAPTAIN_DENIED_COMMANDS.join(",");
        let allowed = format!("--allowedTools={}", CAPTAIN_ALLOWED_TOOLS.join(","));
        assert_eq!(
            claude_launch_args(
                AgentRole::Captain,
                AgentKind::Claude,
                true,
                Some(repo),
                &dirs
            ),
            [
                "--permission-mode=dontAsk".to_string(),
                allowed.clone(),
                format!(
                    "--disallowedTools=NotebookEdit,Edit(//code/api/**),\
                     Edit(//code/api-worktrees/**),Edit(//notes/**),Edit(//docs/**),{denied}"
                ),
                "--add-dir=/notes".into(),
                "--add-dir=/docs".into()
            ]
        );
        assert_eq!(
            claude_launch_args(AgentRole::Captain, AgentKind::Claude, true, None, &[]),
            [
                "--permission-mode=dontAsk".to_string(),
                allowed,
                format!("--disallowedTools=Edit,Write,NotebookEdit,{denied}")
            ],
            "with no repository known, no edits at all"
        );
        assert_eq!(
            claude_launch_args(
                AgentRole::Worker,
                AgentKind::Claude,
                false,
                Some(repo),
                &dirs
            ),
            ["--add-dir=/notes", "--add-dir=/docs"]
        );
        assert!(claude_launch_args(
            AgentRole::Captain,
            AgentKind::Codex,
            false,
            Some(repo),
            &dirs
        )
        .is_empty());
    }

    /// The orchestrator routes project work through captains, with the
    /// commands that get, task and wait on one.
    #[test]
    fn the_orchestrator_guidance_routes_project_work_to_captains() {
        for needle in [
            "nebula orchestrator captain <project-id-or-name> [--json]",
            "`send` the captain the request in the user's words",
            "nebula session wait <captain-id> --until-done",
            "nebula orchestrator read <session-id> [--bytes N | --report]",
            "`read <captain-id> --report` prints that report on its own",
            "`captain <project>` as a background command",
            "\"Claude asks whether to trust <the project's folder>; F9 brings it up\"",
            "Never answer a dialog for the user or offer to",
            "never ask the user to run a `nebula` command",
            "report it as theirs and never disown it",
            "without a captain hop",
            "never change a project's files",
            "only once the earlier captain reports DONE",
            "is done with what you sent it",
            "needs no comment",
        ] {
            assert!(CLAUDE_ORCHESTRATOR_GUIDANCE.contains(needle), "{needle}");
        }
    }

    /// Captain guidance names only commands that exist, with flags they
    /// take, and the report line the board reads.
    #[test]
    fn the_captain_guidance_names_the_commands_it_runs() {
        for needle in [
            "nebula spawn --worktree <branch> --name <short title> --json \
             \"$(cat <<'EOF'\n<task>\nEOF\n)\"",
            "`nebula ask <id> --no-wait --timeout 3600` with the follow-up after it in a quoted heredoc",
            "name each worker by its title",
            "nebula session wait <id> --until-done",
            "nebula read <id> --report",
            "--no-wait",
            "`DONE <what>`, `BLOCKED <why>` or `QUESTION <what>`",
            "`DONE started <what>`",
            "Notes:",
            "at most 4",
            "Never merge",
            "`[nebula] PR #<n>",
            "Your Bash is for reading",
            "never with `git -C`",
            "never look for another way round a denial",
            "Claude memory for this repo",
            "`To record: <the decision>`",
        ] {
            assert!(CLAUDE_CAPTAIN_GUIDANCE.contains(needle), "{needle}");
        }
    }

    #[tokio::test]
    async fn only_the_orchestrator_gets_a_captain_and_never_for_a_missing_checkout() {
        let daemon = daemon();
        let gone = std::path::PathBuf::from("/nonexistent/nebula-captain-test");
        daemon
            .store
            .insert_project(&Project {
                id: ProjectId("gone".into()),
                name: "gone".into(),
                repo_path: gone.clone(),
                sort_order: 0,
            })
            .unwrap();
        daemon
            .store
            .insert_worktree(&Worktree {
                id: WorktreeId("gone".into()),
                project_id: ProjectId("gone".into()),
                path: gone,
                branch: "main".into(),
                is_main: true,
                sort_order: 0,
                removed: false,
            })
            .unwrap();
        for (id, role) in [
            ("worker", AgentRole::Worker),
            ("orchestrator", AgentRole::Orchestrator),
        ] {
            daemon.store.insert_agent(&agent(id, role)).unwrap();
        }
        let project = ProjectId("gone".into());
        let err = daemon
            .orchestrator_captain(&AgentId("worker".into()), &project)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("only available"), "{err}");
        let err = daemon
            .orchestrator_captain(&AgentId("orchestrator".into()), &project)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("gone from disk"), "{err}");
    }

    // The release instance's orchestrator is plain `orchestrator`; a dev
    // one is named for the checkout its data dir is for.
    #[test]
    fn a_dev_instance_names_its_orchestrator_for_its_checkout() {
        assert_eq!(remote_control_name(None), "orchestrator");
        assert_eq!(
            remote_control_name(Some("/Users/me/.nebula-dev/nebula-82beff83")),
            "orchestrator-dev-nebula"
        );
        assert_eq!(
            remote_control_name(Some("/Users/me/.nebula-dev/fix-wait-0a1b2c3d/")),
            "orchestrator-dev-fix-wait"
        );
        // No slot hash: the whole name.
        assert_eq!(
            remote_control_name(Some("/tmp/t/data")),
            "orchestrator-dev-data"
        );
        assert_eq!(
            remote_control_name(Some("/tmp/my-branch")),
            "orchestrator-dev-my-branch"
        );
        assert_eq!(remote_control_name(Some("/")), "orchestrator-dev");
    }

    #[test]
    fn only_orchestrator_role_can_use_privileged_list() {
        let daemon = daemon();
        daemon
            .store
            .insert_agent(&agent("worker", AgentRole::Worker))
            .unwrap();
        let err = daemon
            .orchestrator_tree(&AgentId("worker".into()))
            .unwrap_err();
        assert!(err.to_string().contains("only available"));

        daemon
            .store
            .insert_agent(&agent("orchestrator", AgentRole::Orchestrator))
            .unwrap();
        let (_, _, agents, _, _) = daemon
            .orchestrator_tree(&AgentId("orchestrator".into()))
            .unwrap();
        assert_eq!(agents.len(), 2);
        assert!(agents
            .iter()
            .any(|a| a.id.0 == "orchestrator" && a.role == AgentRole::Orchestrator));
    }
}
