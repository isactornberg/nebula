//! The BOARD: what needs attention across every project, the column the
//! ORCHESTRATOR VIEW draws left of the session on screen. Its rows come in
//! sections, in attention order: `needs you` (NEEDS FEEDBACK), `done`
//! (unseen finishes) and `running`, each session in the PALETTE's
//! attention order within its section, and everything else counted on one
//! line, idle apart from stopped. The orchestrator is not a row: it is the
//! chat.
//!
//! A project with a CAPTAIN is one row, the captain's, in the section its
//! own state puts it, or under `captains` while it is at rest: it stands
//! for the project's other sessions, counting the ones at work, and only
//! the ones that need the user are rows of their own.
//!
//! A `needs you` or `done` row says why under it, from the session's
//! screen ([`nebula_core::why`]): a BOARD READ, asked of the daemon once
//! per state the session is in ([`due_reads`]), never per frame. A finish
//! whose report line is a `QUESTION` or `BLOCKED` waits on the user as a
//! dialog does, so it is a `needs you` row.

use crate::Tree;
use nebula_core::why::{report_word, why_line};
use nebula_core::{Agent, AgentId, AgentStatus, OutputTail, ProjectId};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// The beat a client takes its BOARD READS on ([`due_reads`]): a state
/// begun within one is read on the next.
pub const READ_BEAT: Duration = Duration::from_secs(1);

/// A board section, in the order they are drawn and walked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Section {
    NeedsYou,
    Done,
    Running,
    /// CAPTAINS at rest, which the header does not count: nothing on them
    /// waits on the user.
    Captains,
}

impl Section {
    pub const ALL: [Section; 4] = [
        Section::NeedsYou,
        Section::Done,
        Section::Running,
        Section::Captains,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::NeedsYou => "needs you",
            Section::Done => "done",
            Section::Running => "running",
            Section::Captains => "captains",
        }
    }

    /// What the board's header says after `n` of it: `2 need you`,
    /// `1 needs you`, `1 done`; nothing for a section it does not count.
    pub fn count_word(self, n: usize) -> Option<&'static str> {
        Some(match self {
            Section::NeedsYou if n == 1 => "needs you",
            Section::NeedsYou => "need you",
            Section::Done => "done",
            Section::Running => "running",
            Section::Captains => return None,
        })
    }

    /// Does a row in this section say why under it?
    pub fn reads(self) -> bool {
        matches!(self, Section::NeedsYou | Section::Done)
    }
}

/// One row of the board: a session, the section it is in, what it stands
/// for, and whether it names its checkout on a line under its title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardRow {
    pub id: AgentId,
    pub section: Section,
    pub kind: RowKind,
    pub checkout: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// The session alone.
    Session,
    /// A project's CAPTAIN, standing for the project's other sessions:
    /// `workers` of them are at work, running or waiting on the user.
    Captain { workers: usize },
}

/// The board as the tree has it now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Board {
    /// Section by section, in attention order within each.
    pub rows: Vec<BoardRow>,
    /// Sessions in no section that are up and waiting: fresh, or with a
    /// finish already read.
    pub idle: usize,
    /// Sessions in no section whose process is gone: died while working
    /// (TERMINATED) or lost to a daemon restart (DISCONNECTED).
    pub stopped: usize,
}

impl Board {
    /// The board of the sessions in `order`, the attention order
    /// ([`Tree::board_sessions`]), with `on_screen` the session shown
    /// beside it and `reads` what was read off each one's screen. A project
    /// with a CAPTAIN gives the captain a row whatever its state, and of
    /// its other sessions only the ones that need the user, and the one on
    /// screen, so the lit row is always drawn; the rest it stands for are
    /// on the captain's row or counted with the rest. `names_checkout`
    /// says whether a session's row names its checkout under its title.
    pub fn build(
        tree: &Tree,
        order: &[AgentId],
        on_screen: Option<&AgentId>,
        reads: &HashMap<AgentId, BoardRead>,
        names_checkout: impl Fn(&Agent) -> bool,
    ) -> Board {
        let project_of = |a: &Agent| {
            tree.worktrees
                .iter()
                .find(|w| w.id == a.worktree_id)
                .map(|w| w.project_id.clone())
        };
        let mut by_project: HashMap<ProjectId, Vec<&Agent>> = HashMap::new();
        for a in &tree.agents {
            if let Some(project) = project_of(a) {
                by_project.entry(project).or_default().push(a);
            }
        }
        let captains: HashMap<ProjectId, &AgentId> = by_project
            .into_iter()
            .filter_map(|(project, agents)| Some((project, &nebula_core::live_captain(agents)?.id)))
            .collect();
        let captain_of = |a: &Agent| project_of(a).and_then(|p| captains.get(&p).copied());
        let mut workers: HashMap<&AgentId, usize> = HashMap::new();
        for a in &tree.agents {
            let at_work = matches!(a.status, AgentStatus::Running | AgentStatus::NeedsFeedback);
            match captain_of(a) {
                Some(captain) if *captain != a.id && !a.archived && at_work => {
                    *workers.entry(captain).or_default() += 1
                }
                _ => {}
            }
        }
        let mut board = Board::default();
        for id in order {
            let Some(a) = tree.agents.iter().find(|a| &a.id == id) else {
                continue;
            };
            let on_screen = on_screen == Some(id);
            let captain = captain_of(a);
            let is_captain = captain == Some(id);
            // A captain always has a row, so being on screen never moves it.
            let section = section_of(a, on_screen && !is_captain, read_of(reads, a));
            if is_captain {
                let workers = workers.get(id).copied().unwrap_or_default();
                board.rows.push(BoardRow {
                    section: section.unwrap_or(Section::Captains),
                    kind: RowKind::Captain { workers },
                    checkout: false,
                    id: id.clone(),
                });
                continue;
            }
            let folded = captain.is_some() && section != Some(Section::NeedsYou) && !on_screen;
            match section {
                Some(_) if folded => {}
                Some(section) => board.rows.push(BoardRow {
                    checkout: names_checkout(a),
                    id: id.clone(),
                    section,
                    kind: RowKind::Session,
                }),
                None if matches!(
                    a.status,
                    AgentStatus::Terminated | AgentStatus::Disconnected
                ) =>
                {
                    board.stopped += 1
                }
                None => board.idle += 1,
            }
        }
        // Stable: each section keeps the attention order it came in.
        board.rows.sort_by_key(|r| r.section);
        board
    }

    pub fn count(&self, section: Section) -> usize {
        self.rows.iter().filter(|r| r.section == section).count()
    }

    /// The sessions in no section, counted as the board's last line says
    /// them: `8 idle · 1 stopped`. Empty when there are none.
    pub fn rest(&self) -> String {
        [(self.idle, "idle"), (self.stopped, "stopped")]
            .into_iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, word)| format!("{n} {word}"))
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

/// The section `a` is in, if any, with `read` what the board read off its
/// screen in the state it is in. The session on screen keeps a finished
/// turn's row while it is there: putting it up is what reads it, and its
/// row must not vanish from under the light that says it is the one shown.
/// A finish that reports a `QUESTION` or `BLOCKED` waits on the user, read
/// or not, until the session starts another turn.
fn section_of(a: &Agent, on_screen: bool, read: Option<&BoardRead>) -> Option<Section> {
    match a.status {
        AgentStatus::NeedsFeedback => Some(Section::NeedsYou),
        AgentStatus::Finished if asks(read) => Some(Section::NeedsYou),
        AgentStatus::Finished if a.unseen || on_screen => Some(Section::Done),
        AgentStatus::Running => Some(Section::Running),
        _ => None,
    }
}

/// Does a finish the board read as `read` ask the user something: is its
/// report line a `QUESTION` or `BLOCKED`?
fn asks(read: Option<&BoardRead>) -> bool {
    read.and_then(|r| r.line.as_deref())
        .and_then(report_word)
        .is_some()
}

/// Does `a` wait on the user: a dialog, or a finish whose report line, in
/// the state it is in now, is a `QUESTION` or `BLOCKED`?
pub fn waits_on_user(reads: &HashMap<AgentId, BoardRead>, a: &Agent) -> bool {
    match a.status {
        AgentStatus::NeedsFeedback => true,
        AgentStatus::Finished => asks(read_of(reads, a)),
        _ => false,
    }
}

/// What the board read off `a`'s screen in the state it is in now.
pub fn read_of<'a>(reads: &'a HashMap<AgentId, BoardRead>, a: &Agent) -> Option<&'a BoardRead> {
    reads
        .get(&a.id)
        .filter(|read| read.state == (a.status, a.status_changed_at))
}

/// What the board read off one session's screen, and the state the
/// session was in for it: a session is read again only once that changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardRead {
    /// The session's status and when it moved there.
    pub state: (AgentStatus, i64),
    /// Asked of the daemon already. A state the beat sees begin is asked
    /// after on the next beat, so the screen has had a moment to paint what
    /// the change was about: a permission dialog draws a moment after the
    /// hook that reported it. One that began longer ago is asked at once.
    pub asked: bool,
    /// The line under the row, once the answer is in.
    pub line: Option<String>,
}

/// The sessions among `rows`, the board rows that say why under them, that
/// are due a BOARD READ on this `beat` at `now_ms`, once per state each is
/// in: a state that began more than a beat ago is read at once, and one
/// the beat sees begin is noted in `reads` and read on the beat after
/// ([`BoardRead::asked`]). A row listed twice is read once. A read lasts
/// while its session stays in that state, so `reads` drops the ones whose
/// session moved on. Each session returned is asked for its screen
/// (`TailOutput` of [`SCREEN_TAIL_BYTES`](crate::screen::SCREEN_TAIL_BYTES)),
/// and the answer goes to [`land_read`].
pub fn due_reads(
    reads: &mut HashMap<AgentId, BoardRead>,
    agents: &[Agent],
    rows: &[AgentId],
    now_ms: i64,
    beat: Duration,
) -> Vec<AgentId> {
    reads.retain(|id, read| {
        agents
            .iter()
            .any(|a| a.id == *id && (a.status, a.status_changed_at) == read.state)
    });
    let settled_before = now_ms - beat.as_millis() as i64;
    let mut seen = HashSet::new();
    let mut due = Vec::new();
    for id in rows {
        if !seen.insert(id) {
            continue;
        }
        let Some(a) = agents.iter().find(|a| &a.id == id) else {
            continue;
        };
        let state = (a.status, a.status_changed_at);
        let settled = a.status_changed_at < settled_before;
        match reads.get_mut(id) {
            Some(read) if read.asked => continue,
            Some(read) => read.asked = true,
            None => {
                reads.insert(
                    id.clone(),
                    BoardRead {
                        state,
                        asked: settled,
                        line: None,
                    },
                );
                if !settled {
                    continue;
                }
            }
        }
        due.push(id.clone());
    }
    due
}

/// The daemon's answer to a read [`due_reads`] asked for: the end of the
/// session's ring (`None` when it has none), laid out as the PTY has it and
/// read for the line under its row. Whether that line changed.
pub fn land_read(
    reads: &mut HashMap<AgentId, BoardRead>,
    id: &AgentId,
    tail: Option<&OutputTail>,
) -> bool {
    // A state not asked after yet: the answer is to an ask from before it.
    let Some(read) = reads.get_mut(id).filter(|r| r.asked) else {
        return false;
    };
    let rows = tail
        .map(|t| crate::screen::screen_text(&t.data, t.cols, t.rows))
        .unwrap_or_default();
    let line = why_line(read.state.0, &rows);
    let changed = read.line != line;
    read.line = line;
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::{AgentKind, AgentRole, WorktreeId};

    #[test]
    fn the_rest_counts_idle_apart_from_stopped() {
        let board = |idle, stopped| Board {
            rows: Vec::new(),
            idle,
            stopped,
        };
        assert_eq!(board(8, 2).rest(), "8 idle · 2 stopped");
        assert_eq!(board(0, 1).rest(), "1 stopped");
        assert_eq!(board(3, 0).rest(), "3 idle");
        assert_eq!(board(0, 0).rest(), "");
    }

    fn agent(id: &str, status: AgentStatus, changed_at: i64) -> Agent {
        Agent {
            id: AgentId(id.into()),
            worktree_id: WorktreeId("w".into()),
            name: id.into(),
            status,
            archived: false,
            archived_at: 0,
            unseen: false,
            status_changed_at: changed_at,
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

    fn tail(lines: &[&str]) -> OutputTail {
        OutputTail {
            cols: 60,
            rows: lines.len() as u16,
            end_seq: 1,
            data: lines.join("\r\n").into_bytes(),
        }
    }

    /// A state the beat sees begin is read on the beat after, one that
    /// began longer ago at once, each once per state, and a read lasts
    /// only while its session stays in the state it was taken in.
    #[test]
    fn a_row_is_read_once_per_state_a_beat_after_it_began() {
        let beat = Duration::from_secs(1);
        let now = 100_000;
        let mut reads = HashMap::new();
        let mut agents = vec![
            agent("new", AgentStatus::NeedsFeedback, now - 10),
            agent("old", AgentStatus::Finished, now - 60_000),
        ];
        let ids = |ids: &[&str]| {
            ids.iter()
                .map(|id| AgentId((*id).into()))
                .collect::<Vec<_>>()
        };
        let rows = ids(&["new", "old", "old", "gone"]);
        assert_eq!(
            due_reads(&mut reads, &agents, &rows, now, beat),
            ids(&["old"])
        );
        assert_eq!(
            due_reads(&mut reads, &agents, &rows, now, beat),
            ids(&["new"])
        );
        assert!(due_reads(&mut reads, &agents, &rows, now, beat).is_empty());

        let old = AgentId("old".into());
        assert!(land_read(
            &mut reads,
            &old,
            Some(&tail(&["⏺ QUESTION: which base?"]))
        ));
        assert!(!land_read(
            &mut reads,
            &old,
            Some(&tail(&["⏺ QUESTION: which base?"]))
        ));
        assert_eq!(
            read_of(&reads, &agents[1]).and_then(|r| r.line.as_deref()),
            Some("QUESTION: which base?")
        );
        assert!(waits_on_user(&reads, &agents[1]), "a finish that asks");

        agents[1] = agent("old", AgentStatus::Running, now);
        assert!(read_of(&reads, &agents[1]).is_none(), "another state");
        assert!(!waits_on_user(&reads, &agents[1]));
        assert!(due_reads(&mut reads, &agents, &rows, now, beat).is_empty());
        assert!(
            !land_read(&mut reads, &old, Some(&tail(&["late"]))),
            "an answer to an ask from the state before is dropped"
        );
    }
}
