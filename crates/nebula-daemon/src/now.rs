//! The NOW BLOCK: what the daemon adds to every prompt of the ORCHESTRATOR
//! and of a CAPTAIN, the user's and the ones nebula types alike (a done
//! note, a merge note, a brief, the rank turn), in the UserPromptSubmit
//! hook's `additionalContext` ([`crate::hooks::user_prompt_reply`]): the
//! state as the prompt was sent, so a question about where things stand is
//! answered from it, with no tool call and nothing taken from memory.
//!
//! The orchestrator's is the BOARD, every project's: its rows one line
//! each, a project with a captain as the captain's row, and a line
//! counting the rest. A captain's is its project's other sessions one line
//! each, and a line on its own checkout from git's local refs: its branch
//! and HEAD, what it has uncommitted, and how far it is from its base on
//! origin as of the last fetch. Pull requests are left out: the TUI and
//! `nebula serve` hold their state, not the daemon, and the captain asks
//! `gh pr view`.
//!
//! The hook holds the prompt while it waits, so the block is given up past
//! [`BUDGET`] and the checkout line past [`CHECKOUT_BUDGET`], nothing is
//! fetched, and a block that fails to build is left out.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use nebula_client::board::{age, read_of, Board, BoardRead, RowKind};
use nebula_client::Tree;
use nebula_core::why::{screen_text, why_line};
use nebula_core::{Agent, AgentId, AgentKind, AgentRole, AgentStatus, ProjectId, SessionRef};

use crate::git::CheckoutState;
use crate::registry::Daemon;

/// What the block opens with.
pub const HEADER: &str = "[nebula now]";
/// The most sessions the block names; the rest are counted.
const ROWS: usize = 12;
/// The longest a why line runs in the block, in characters.
const WHY_CHARS: usize = 160;
/// How long the block may take: the hook's `curl` gives up at 3 s.
const BUDGET: Duration = Duration::from_secs(1);
/// How long git may take to say how the captain's checkout stands.
const CHECKOUT_BUDGET: Duration = Duration::from_millis(500);

impl Daemon {
    /// The NOW BLOCK for a prompt of session `id`: `None` but for the
    /// ORCHESTRATOR or a CAPTAIN on Claude, when there is nothing to say,
    /// and when it could not be built in time.
    pub(crate) async fn now_block(self: &Arc<Self>, id: &AgentId) -> Option<String> {
        let me = self.store.get_agent(id).ok()??;
        if me.archived || me.kind != AgentKind::Claude || me.role == AgentRole::Worker {
            return None;
        }
        tokio::time::timeout(BUDGET, self.build_now(me))
            .await
            .ok()?
    }

    async fn build_now(self: &Arc<Self>, me: Agent) -> Option<String> {
        let daemon = self.clone();
        let looker = me.clone();
        let (tree, reads) = tokio::task::spawn_blocking(move || daemon.now_tree(&looker))
            .await
            .ok()??;
        let now = nebula_core::clock::now_ms();
        if me.role == AgentRole::Orchestrator {
            return orchestrator_block(&tree, &reads, now);
        }
        let root = tree.filed_under(&me.worktree_id)?;
        let repo = &tree
            .projects
            .iter()
            .find(|p| p.id == root.project_id)?
            .repo_path;
        let configured = crate::config::Config::load()
            .worktree_base_branch()
            .map(str::to_string);
        let checkout = tokio::time::timeout(
            CHECKOUT_BUDGET,
            crate::git::checkout_state(repo, &root.path, configured.as_deref()),
        )
        .await
        .ok()
        .flatten();
        captain_block(&tree, &me, &reads, checkout.as_ref(), now)
    }

    /// The tree as the store has it, and what the screen of each session
    /// `me` sees that may say why says: one waiting on the user or
    /// finished, in any project for the orchestrator and in its own for a
    /// captain.
    // ponytail: every such screen is read on each prompt, no cache; one
    // keyed on the session's state would do if many live finished
    // sessions ever make the hook slow.
    fn now_tree(&self, me: &Agent) -> Option<(Tree, HashMap<AgentId, BoardRead>)> {
        let (projects, worktrees, agents, terminals) = self.store.load_tree().ok()?;
        let mut tree = Tree {
            projects,
            agents,
            terminals,
            ..Tree::default()
        };
        tree.set_worktrees(worktrees);
        let scope = match me.role {
            AgentRole::Captain => Some(project_of(&tree, me)?),
            _ => None,
        };
        let reads = tree
            .agents
            .iter()
            .filter(|a| a.id != me.id && !a.archived)
            .filter(|a| matches!(a.status, AgentStatus::NeedsFeedback | AgentStatus::Finished))
            .filter(|a| scope.is_none() || project_of(&tree, a) == scope)
            .filter_map(|a| {
                let session = self.session(&SessionRef::Agent(a.id.clone()))?;
                let tail = session.tail(crate::orchestrator::NOTE_SCREEN_BYTES, None);
                let rows = screen_text(&tail.data, tail.cols, tail.rows);
                let read = BoardRead {
                    state: (a.status, a.status_changed_at),
                    asked: true,
                    line: why_line(a.status, &rows),
                };
                Some((a.id.clone(), read))
            })
            .collect();
        Some((tree, reads))
    }
}

/// The ORCHESTRATOR's block: the board's rows, most urgent first, at most
/// [`ROWS`] of them, and a line counting what it does not name. `None`
/// with no session to speak of.
pub(crate) fn orchestrator_block(
    tree: &Tree,
    reads: &HashMap<AgentId, BoardRead>,
    now: i64,
) -> Option<String> {
    let board = Board::build(tree, &tree.board_sessions(), None, reads, |_| false);
    let cut = board.rows.len().saturating_sub(ROWS);
    let rest: Vec<String> = (cut > 0)
        .then(|| format!("{cut} more {} not shown", plural(cut, "row")))
        .into_iter()
        .chain(Some(board.rest()).filter(|r| !r.is_empty()))
        .collect();
    if board.rows.is_empty() && rest.is_empty() {
        return None;
    }
    let mut out = vec![if board.rows.is_empty() {
        format!(
            "{HEADER} The board as this prompt was sent: no session needs the user, is done or is \
             running."
        )
    } else {
        format!(
            "{HEADER} The board as this prompt was sent, a session a line (section | session | \
             project | title | status, how long in it | why):"
        )
    }];
    for row in board.rows.iter().take(ROWS) {
        let Some(a) = tree.agents.iter().find(|a| a.id == row.id) else {
            continue;
        };
        let title = match row.kind {
            RowKind::Captain { workers } => {
                format!("captain, {workers} {} at work", plural(workers, "worker"))
            }
            RowKind::Session => one_line(&a.name),
        };
        let project = project_name(tree, a);
        out.push(row_line(
            &[
                row.section.title(),
                a.id.as_str(),
                &project,
                &title,
                &status(a, now),
            ],
            reads,
            a,
        ));
    }
    if !rest.is_empty() {
        out.push(format!("The rest: {}", rest.join(" · ")));
    }
    Some(out.join("\n"))
}

/// A CAPTAIN's block: the other sessions in its project, most urgent
/// first, at most [`ROWS`] of them, and how its checkout stands, when git
/// said in time. `None` with neither.
pub(crate) fn captain_block(
    tree: &Tree,
    me: &Agent,
    reads: &HashMap<AgentId, BoardRead>,
    checkout: Option<&CheckoutState>,
    now: i64,
) -> Option<String> {
    let project = project_of(tree, me)?;
    let others: Vec<&Agent> = tree
        .attention_sessions()
        .iter()
        .filter_map(|id| tree.agents.iter().find(|a| &a.id == id))
        .filter(|a| a.id != me.id && project_of(tree, a).as_ref() == Some(&project))
        .collect();
    if others.is_empty() && checkout.is_none() {
        return None;
    }
    let mut out = vec![if others.is_empty() {
        format!("{HEADER} This project as this prompt was sent: no other sessions.")
    } else {
        format!(
            "{HEADER} This project as this prompt was sent, its other sessions a line each \
             (session | title | status, how long in it | branch | why):"
        )
    }];
    for a in others.iter().take(ROWS) {
        let branch = tree
            .filed_under(&a.worktree_id)
            .map(|w| w.branch.clone())
            .unwrap_or_default();
        out.push(row_line(
            &[a.id.as_str(), &one_line(&a.name), &status(a, now), &branch],
            reads,
            a,
        ));
    }
    let cut = others.len().saturating_sub(ROWS);
    if cut > 0 {
        out.push(format!(
            "The rest: {cut} more {} not shown",
            plural(cut, "session")
        ));
    }
    if let Some(checkout) = checkout {
        out.push(checkout_line(checkout, now));
    }
    Some(out.join("\n"))
}

/// The checkout line of a CAPTAIN's block: `Checkout: on main at bdb0624,
/// 2 uncommitted files, 1 commit ahead of origin/main and 3 behind it, as
/// of the last fetch, 2h ago.`
pub(crate) fn checkout_line(c: &CheckoutState, now: i64) -> String {
    let on = match &c.branch {
        Some(branch) => format!("on {branch} at {}", c.head),
        None => format!("on a detached HEAD at {}", c.head),
    };
    let files = match c.uncommitted {
        0 => "nothing uncommitted".to_string(),
        n => format!("{n} uncommitted {}", plural(n, "file")),
    };
    let gap = match &c.base {
        None => "no branch on origin to compare it with".to_string(),
        Some(g) => {
            let commits = |n: u32| format!("{n} {}", plural(n as usize, "commit"));
            let gap = match (g.ahead, g.behind) {
                (0, 0) => format!("even with {}", g.name),
                (ahead, 0) => format!("{} ahead of {}", commits(ahead), g.name),
                (0, behind) => format!("{} behind {}", commits(behind), g.name),
                (ahead, behind) => {
                    format!(
                        "{} ahead of {} and {behind} behind it",
                        commits(ahead),
                        g.name
                    )
                }
            };
            match g.fetched_at.map(|at| age(at, now)) {
                Some(ago) if ago == "now" => format!("{gap}, as of the last fetch, just now"),
                Some(ago) => format!("{gap}, as of the last fetch, {ago} ago"),
                None => format!("{gap}, with no fetch on record"),
            }
        }
    };
    format!("Checkout: {on}, {files}, {gap}.")
}

/// One session's line: `fields` and, when the session says why it is
/// where it is, that, cut to [`WHY_CHARS`].
fn row_line(fields: &[&str], reads: &HashMap<AgentId, BoardRead>, a: &Agent) -> String {
    let mut line = format!("- {}", fields.join(" | "));
    if let Some(why) = read_of(reads, a).and_then(|r| r.line.as_deref()) {
        line.push_str(" | ");
        if why.chars().count() > WHY_CHARS {
            line.extend(why.chars().take(WHY_CHARS - 1));
            line.push('…');
        } else {
            line.push_str(why);
        }
    }
    line
}

/// `running 4m`: a session's status and how long it has been in it.
fn status(a: &Agent, now: i64) -> String {
    format!("{} {}", a.status.as_str(), age(a.status_changed_at, now))
}

fn project_of(tree: &Tree, a: &Agent) -> Option<ProjectId> {
    tree.filed_under(&a.worktree_id)
        .map(|w| w.project_id.clone())
}

fn project_name(tree: &Tree, a: &Agent) -> String {
    project_of(tree, a)
        .and_then(|id| tree.projects.iter().find(|p| p.id == id))
        .map(|p| one_line(&p.name))
        .unwrap_or_default()
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::BaseGap;
    use nebula_core::{Project, Worktree, WorktreeId};

    const NOW: i64 = 100_000_000;
    const MIN: i64 = 60_000;

    fn agent(id: &str, worktree: &str, name: &str, status: AgentStatus, ago: i64) -> Agent {
        Agent {
            id: AgentId(id.into()),
            worktree_id: WorktreeId(worktree.into()),
            name: name.into(),
            status,
            archived: false,
            archived_at: 0,
            unseen: false,
            status_changed_at: NOW - ago,
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            issue_url: None,
            sort_order: 0,
            alive: true,
            role: AgentRole::Worker,
            recent_prompts: Vec::new(),
        }
    }

    fn captain(id: &str, worktree: &str, status: AgentStatus, ago: i64) -> Agent {
        Agent {
            role: AgentRole::Captain,
            ..agent(id, worktree, "captain", status, ago)
        }
    }

    /// Two projects, `shopfront` with no captain and `api` with one, each
    /// with its root checkout on `main` and a worktree.
    fn tree(agents: Vec<Agent>) -> Tree {
        let project = |id: &str, name: &str| Project {
            id: ProjectId(id.into()),
            name: name.into(),
            repo_path: format!("/code/{name}").into(),
            sort_order: 0,
        };
        let worktree = |id: &str, project: &str, branch: &str| Worktree {
            id: WorktreeId(id.into()),
            project_id: ProjectId(project.into()),
            path: format!("/code/{id}").into(),
            branch: branch.into(),
            is_main: branch == "main",
            sort_order: 0,
            removed: false,
        };
        let mut tree = Tree {
            projects: vec![project("p1", "shopfront"), project("p2", "api")],
            agents,
            ..Tree::default()
        };
        tree.set_worktrees(vec![
            worktree("w1", "p1", "main"),
            worktree("w2", "p1", "login-redirect"),
            worktree("w3", "p2", "main"),
            worktree("w4", "p2", "fix-flaky"),
        ]);
        tree
    }

    /// What was read off `a`'s screen in the state it is in.
    fn read(a: &Agent, line: &str) -> (AgentId, BoardRead) {
        let read = BoardRead {
            state: (a.status, a.status_changed_at),
            asked: true,
            line: Some(line.into()),
        };
        (a.id.clone(), read)
    }

    /// The orchestrator's block is the board: what needs the user first,
    /// with what it asks, then what runs, then each project with a captain
    /// as the captain's row counting its workers at work, which have no
    /// row of their own unless they need the user, and the rest counted.
    #[test]
    fn the_orchestrator_block_is_the_board_with_a_row_for_each_captain() {
        let s1 = agent(
            "s1",
            "w2",
            "Login redirect",
            AgentStatus::NeedsFeedback,
            3 * MIN,
        );
        let a2 = agent("a2", "w4", "Docs pass", AgentStatus::NeedsFeedback, MIN);
        let tree = tree(vec![
            Agent {
                role: AgentRole::Orchestrator,
                ..agent("o1", "w1", "orchestrator", AgentStatus::Running, 0)
            },
            s1.clone(),
            agent("s2", "w1", "Checkout flow", AgentStatus::Running, 4 * MIN),
            agent("s3", "w1", "Old idea", AgentStatus::Finished, 120 * MIN),
            agent("s4", "w1", "Crashed", AgentStatus::Terminated, 10 * MIN),
            captain("c1", "w3", AgentStatus::Finished, 60 * MIN),
            agent("a1", "w4", "Fix flaky test", AgentStatus::Running, 4 * MIN),
            a2.clone(),
        ]);
        let reads = HashMap::from([read(&s1, "Bash: cargo build")]);
        assert_eq!(
            orchestrator_block(&tree, &reads, NOW).unwrap(),
            "[nebula now] The board as this prompt was sent, a session a line (section | session \
             | project | title | status, how long in it | why):\n\
             - needs you | s1 | shopfront | Login redirect | needs_feedback 3m | Bash: cargo build\n\
             - needs you | a2 | api | Docs pass | needs_feedback 1m\n\
             - running | s2 | shopfront | Checkout flow | running 4m\n\
             - captains | c1 | api | captain, 2 workers at work | finished 1h\n\
             The rest: 1 idle · 1 stopped"
        );
    }

    /// A finish whose report line asks the user is among what needs them,
    /// the rows past the cap are counted, and with no session at all
    /// there is no block.
    #[test]
    fn the_orchestrator_block_is_capped_and_left_out_with_nothing_to_say() {
        let asks = agent("q1", "w1", "Pick a base", AgentStatus::Finished, 2 * MIN);
        let mut agents: Vec<Agent> = (0..14)
            .map(|n| agent(&format!("r{n:02}"), "w1", "Work", AgentStatus::Running, MIN))
            .collect();
        agents.push(asks.clone());
        let reads = HashMap::from([read(&asks, "QUESTION which base branch?")]);
        let block = orchestrator_block(&tree(agents), &reads, NOW).unwrap();
        let lines: Vec<&str> = block.lines().collect();
        assert_eq!(lines.len(), 1 + ROWS + 1, "{block}");
        assert_eq!(
            lines[1],
            "- needs you | q1 | shopfront | Pick a base | finished 2m | QUESTION which base branch?"
        );
        assert_eq!(lines[ROWS + 1], "The rest: 3 more rows not shown");

        assert_eq!(orchestrator_block(&tree(Vec::new()), &reads, NOW), None);
        assert_eq!(
            orchestrator_block(
                &tree(vec![agent("i1", "w1", "Idle", AgentStatus::Fresh, MIN)]),
                &HashMap::new(),
                NOW
            )
            .unwrap(),
            "[nebula now] The board as this prompt was sent: no session needs the user, is done \
             or is running.\nThe rest: 1 idle"
        );
    }

    fn checkout(base: Option<BaseGap>) -> CheckoutState {
        CheckoutState {
            branch: Some("main".into()),
            head: "bdb0624".into(),
            uncommitted: 2,
            base,
        }
    }

    fn gap(ahead: u32, behind: u32, fetched_ago: Option<i64>) -> Option<BaseGap> {
        Some(BaseGap {
            name: "origin/main".into(),
            ahead,
            behind,
            fetched_at: fetched_ago.map(|ago| NOW - ago),
        })
    }

    /// A captain's block is its project's other sessions, most urgent
    /// first, each with its branch and what it says, and its checkout;
    /// another project's sessions are not in it.
    #[test]
    fn a_captains_block_is_its_projects_sessions_and_its_checkout() {
        let me = captain("c1", "w3", AgentStatus::Running, 0);
        let done = agent("a3", "w3", "Typo", AgentStatus::Finished, 12 * MIN);
        let long = "x".repeat(WHY_CHARS + 20);
        let tree = tree(vec![
            me.clone(),
            agent("a1", "w4", "Fix flaky test", AgentStatus::Running, 4 * MIN),
            agent("a2", "w4", "Docs pass", AgentStatus::NeedsFeedback, MIN),
            done.clone(),
            agent("s1", "w1", "Elsewhere", AgentStatus::Running, MIN),
        ]);
        let reads = HashMap::from([read(&done, &long)]);
        let block = captain_block(
            &tree,
            &me,
            &reads,
            Some(&checkout(gap(1, 3, Some(120 * MIN)))),
            NOW,
        )
        .unwrap();
        let clipped = format!("{}…", "x".repeat(WHY_CHARS - 1));
        assert_eq!(
            block,
            format!(
                "[nebula now] This project as this prompt was sent, its other sessions a line each \
                 (session | title | status, how long in it | branch | why):\n\
                 - a2 | Docs pass | needs_feedback 1m | fix-flaky\n\
                 - a1 | Fix flaky test | running 4m | fix-flaky\n\
                 - a3 | Typo | finished 12m | main | {clipped}\n\
                 Checkout: on main at bdb0624, 2 uncommitted files, 1 commit ahead of origin/main \
                 and 3 behind it, as of the last fetch, 2h ago."
            )
        );
    }

    /// Past the cap the rest are counted; a captain alone says so over its
    /// checkout, and with neither sessions nor a checkout line there is no
    /// block.
    #[test]
    fn a_captains_block_is_capped_and_left_out_with_nothing_to_say() {
        let me = captain("c1", "w3", AgentStatus::Running, 0);
        let mut agents: Vec<Agent> = (0..13)
            .map(|n| agent(&format!("a{n:02}"), "w4", "Work", AgentStatus::Running, MIN))
            .collect();
        agents.push(me.clone());
        let block = captain_block(&tree(agents), &me, &HashMap::new(), None, NOW).unwrap();
        assert_eq!(block.lines().count(), 1 + ROWS + 1, "{block}");
        assert!(
            block.ends_with("\nThe rest: 1 more session not shown"),
            "{block}"
        );

        let alone = tree(vec![me.clone()]);
        assert_eq!(captain_block(&alone, &me, &HashMap::new(), None, NOW), None);
        assert_eq!(
            captain_block(&alone, &me, &HashMap::new(), Some(&checkout(None)), NOW).unwrap(),
            "[nebula now] This project as this prompt was sent: no other sessions.\n\
             Checkout: on main at bdb0624, 2 uncommitted files, no branch on origin to compare it \
             with."
        );
    }

    /// The checkout line says how far the checkout is from its base as of
    /// the last fetch, and says so when there has been none.
    #[test]
    fn the_checkout_line_dates_the_gap_by_the_last_fetch() {
        let line = |c: CheckoutState| checkout_line(&c, NOW);
        assert_eq!(
            line(checkout(gap(0, 0, None))),
            "Checkout: on main at bdb0624, 2 uncommitted files, even with origin/main, with no \
             fetch on record."
        );
        assert_eq!(
            line(checkout(gap(2, 0, Some(10_000)))),
            "Checkout: on main at bdb0624, 2 uncommitted files, 2 commits ahead of origin/main, \
             as of the last fetch, just now."
        );
        assert_eq!(
            line(CheckoutState {
                branch: None,
                uncommitted: 0,
                ..checkout(gap(0, 1, Some(3 * 86_400_000)))
            }),
            "Checkout: on a detached HEAD at bdb0624, nothing uncommitted, 1 commit behind \
             origin/main, as of the last fetch, 3d ago."
        );
    }
}
