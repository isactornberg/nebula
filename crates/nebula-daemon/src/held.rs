//! HELD TURNS: the turns the daemon types into a CAPTAIN or the
//! ORCHESTRATOR that nobody at its keys typed, each held until the session
//! is free to take it: a Start's BRIEF (`start_issue`) and the MERGE NOTE a
//! TUI hands over once it sees a pull request merge
//! (`ClientRequest::MergeNote`) for a captain, the RANK TURN the web app
//! or a schedule asks for (`ClientRequest::RankFocus`) for the
//! orchestrator, and a same-session SCHEDULE's prompt for its session
//! (`schedule`).
//!
//! None is typed into a session at work, into a prompt of the user's, or in
//! place of the answer to a question the session asked the user: a held
//! turn goes once the session is at rest with its input box empty and its
//! last turn asking nothing (`Daemon::when_ready`, the wait a DONE NOTE
//! has), oldest first, and the next only once the session has taken the
//! last. A brief is for the captain Start got or started, and goes with it;
//! a merge note is for its project, and goes to whichever captain the
//! project has when the note goes; a rank turn goes with its orchestrator,
//! and a schedule's turn with its session.
//! Held turns are kept in `held-turns.json` in the data dir, so a daemon
//! restart delivers them too, and every subscriber is told which briefs are
//! held (`ServerEvent::HeldBriefs`) and where the rank turn stands
//! (`ServerEvent::RankTurn`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use nebula_core::{AgentId, AgentRole, HeldBrief, ProjectId, RankTurn, ServerEvent, SessionRef};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::registry::Daemon;

/// The RANK TURN: what the ORCHESTRATOR is typed when the web app asks it
/// to rank the focus picks. Its guidance says the rest.
pub(crate) const RANK_TURN: &str = "[nebula] Rank the focus picks for nebula's web app: run \
nebula orchestrator attention --json, choose at most three, and set them with nebula orchestrator \
picks set --json -, as your guidance says. Reply in one line.";

/// The turns held, and the file they are kept in.
#[derive(Default)]
pub(crate) struct Held {
    turns: Vec<Owed>,
    /// None until the daemon restores them as it boots, and in tests, where
    /// nothing is written.
    file: Option<PathBuf>,
    /// When a RANK TURN was last typed, epoch ms.
    rank_typed: Option<i64>,
}

/// A held turn: the text typed into `to` once it is free.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Owed {
    /// The session it waits for, a captain or the orchestrator: a merge
    /// note's moves on to its project's next captain should this one go
    /// ([`Daemon::redirect`]).
    #[serde(rename = "captain")]
    pub(crate) to: AgentId,
    /// When it was handed over, epoch ms.
    pub(crate) at: i64,
    pub(crate) about: About,
    pub(crate) text: String,
}

/// What a held turn is for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum About {
    /// A Start's brief, for the issue at this URL.
    Brief(String),
    /// A merge note, for the pull request at `pr_url`, to `project`'s
    /// captain.
    MergeNote { pr_url: String, project: ProjectId },
    /// The RANK TURN, for the orchestrator.
    Rank,
    /// A run of the same-session SCHEDULE `schedule`: its prompt typed, or
    /// with `fresh` the session's CLI started again on a new conversation
    /// with the prompt its first.
    Schedule { schedule: String, fresh: bool },
}

impl About {
    /// The role of the session a turn about this is typed into.
    fn role(&self) -> AgentRole {
        match self {
            About::Rank => AgentRole::Orchestrator,
            About::Brief(_) | About::MergeNote { .. } => AgentRole::Captain,
            About::Schedule { .. } => AgentRole::Worker,
        }
    }
}

impl Held {
    /// What every subscriber is told of them: the briefs.
    pub(crate) fn event(&self) -> ServerEvent {
        let briefs = self
            .turns
            .iter()
            .filter_map(|o| match &o.about {
                About::Brief(url) => Some(HeldBrief {
                    issue_url: url.clone(),
                    captain: o.to.clone(),
                    at: o.at,
                }),
                About::MergeNote { .. } | About::Rank | About::Schedule { .. } => None,
            })
            .collect();
        ServerEvent::HeldBriefs { briefs }
    }

    /// What every subscriber is told of the RANK TURN.
    pub(crate) fn rank_event(&self) -> ServerEvent {
        ServerEvent::RankTurn {
            turn: RankTurn {
                held_at: self
                    .turns
                    .iter()
                    .find(|o| o.about == About::Rank)
                    .map(|o| o.at),
                typed_at: self.rank_typed,
            },
        }
    }

    /// Whether `owed` is next for its captain, the oldest held for it;
    /// `None` once it is held no more.
    fn next(&self, owed: &Owed) -> Option<bool> {
        self.turns
            .contains(owed)
            .then(|| self.turns.iter().find(|o| o.to == owed.to) == Some(owed))
    }

    fn drop_turn(&mut self, owed: &Owed, daemon: &Daemon) {
        self.turns.retain(|o| o != owed);
        self.changed(daemon);
    }

    /// `owed` is typed now: it is held no more.
    fn typing(&mut self, owed: &Owed, daemon: &Daemon) {
        if owed.about == About::Rank {
            self.rank_typed = Some(nebula_core::clock::now_ms());
        }
        self.drop_turn(owed, daemon);
    }

    /// `owed`, held for `captain` from now on, in its place among the
    /// others; `None` once it is held no more.
    fn retarget(&mut self, owed: &Owed, captain: AgentId, daemon: &Daemon) -> Option<Owed> {
        let held = self.turns.iter_mut().find(|o| *o == owed)?;
        if held.to == captain {
            return Some(held.clone());
        }
        held.to = captain;
        let moved = held.clone();
        self.changed(daemon);
        Some(moved)
    }

    /// Write the file again, renamed into place so a reader never sees half
    /// of it, and tell every subscriber, both under the lock the change was
    /// made in, so they land in the order the changes did.
    fn changed(&self, daemon: &Daemon) {
        if let Some(file) = &self.file {
            if let Err(err) = write(file, &self.turns) {
                tracing::warn!(file = %file.display(), "held turns not written: {err:#}");
            }
        }
        daemon.broadcast(self.event());
        daemon.broadcast(self.rank_event());
    }
}

fn write(file: &Path, turns: &[Owed]) -> Result<()> {
    let tmp = file.with_extension(format!("json.{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(turns)?)?;
    std::fs::rename(&tmp, file)?;
    Ok(())
}

/// The held turns `file` keeps: none when there is no file, and none, said
/// once, when it does not read.
fn read(file: &Path) -> Vec<Owed> {
    match std::fs::read(file) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
            tracing::warn!(file = %file.display(), "held turns not read: {err}");
            Vec::new()
        }),
        Err(_) => Vec::new(),
    }
}

impl Daemon {
    /// Hold `owed` until its session takes it ([`Self::deliver`]), and tell
    /// `taken` when it does; false when it joins one held already. A merge
    /// note for a pull request one is held for already is not held again,
    /// as two TUIs can see one merge, and neither is a second rank turn or
    /// a second turn of one schedule.
    pub(crate) fn hold(self: &Arc<Self>, owed: Owed, taken: Option<oneshot::Sender<()>>) -> bool {
        {
            let mut work = self.sent_work.lock();
            let held = &mut work.held;
            let once = |about: &About| match (about, &owed.about) {
                (About::Schedule { schedule: a, .. }, About::Schedule { schedule: b, .. }) => {
                    a == b
                }
                (about, mine) => {
                    matches!(mine, About::MergeNote { .. } | About::Rank) && about == mine
                }
            };
            if held.turns.iter().any(|o| once(&o.about)) {
                return false;
            }
            held.turns.push(owed.clone());
            held.changed(self);
        }
        let daemon = self.clone();
        tokio::spawn(async move { daemon.deliver(owed, taken).await });
        true
    }

    /// Whether a turn of the schedule `schedule` is held.
    pub(crate) fn schedule_held(&self, schedule: &str) -> bool {
        self.sent_work
            .lock()
            .held
            .turns
            .iter()
            .any(|o| matches!(&o.about, About::Schedule { schedule: s, .. } if s == schedule))
    }

    /// Type `owed` into its session once that is ready for a turn and
    /// `owed` is the oldest it is owed ([`Self::when_ready`]), then tell
    /// `taken`. A session archived or deleted before that takes a brief or
    /// a rank turn with it, and hands a merge note on ([`Self::redirect`]).
    async fn deliver(self: &Arc<Self>, mut owed: Owed, taken: Option<oneshot::Sender<()>>) {
        let session = loop {
            let ready = self
                .when_ready(
                    &owed.to,
                    owed.about.role(),
                    |work| work.held.next(&owed),
                    |work| work.held.typing(&owed, self),
                )
                .await;
            if let Some(session) = ready {
                break session;
            }
            match self.redirect(&owed).await {
                Some(moved) => owed = moved,
                None => return,
            }
        };
        if let Some(taken) = taken {
            let _ = taken.send(());
        }
        let typed = match &owed.about {
            About::Schedule { fresh: true, .. } => {
                drop(session);
                self.restart_fresh(&owed.to, &owed.text)
            }
            _ => self
                .type_turn(&owed.to, &session, &owed.text)
                .await
                .map(drop),
        };
        if let Err(err) = &typed {
            tracing::warn!(to = %owed.to, "held turn typed, not taken whole: {err:#}");
        }
        match &owed.about {
            About::Schedule { schedule, .. } => {
                self.schedule_turn_went(Some(schedule), &owed.to, typed.err())
            }
            About::Rank => self.schedule_turn_went(None, &owed.to, None),
            About::Brief(_) | About::MergeNote { .. } => {}
        }
    }

    /// Where `owed` goes once [`Self::when_ready`] gave up on its session: a
    /// brief or a rank turn nowhere, as it goes with its session; a merge
    /// note to the captain its project has now, as `captain` finds it,
    /// waiting while the project has none. `None` once it is held no more,
    /// or goes nowhere, which is said in the log: a brief, a rank turn, and
    /// a merge note whose project is gone.
    async fn redirect(self: &Arc<Self>, owed: &Owed) -> Option<Owed> {
        let project = match &owed.about {
            About::MergeNote { project, .. } => project,
            About::Brief(_) => {
                self.drop_held(owed, "a held brief went with its captain");
                return None;
            }
            About::Rank => {
                self.drop_held(owed, "a held rank turn went with its orchestrator");
                self.schedule_turn_went(
                    None,
                    &owed.to,
                    Some(anyhow::anyhow!(
                        "the orchestrator went before it took the rank turn"
                    )),
                );
                return None;
            }
            About::Schedule { schedule, .. } => {
                self.drop_held(owed, "a held scheduled turn went with its session");
                self.schedule_turn_went(
                    Some(schedule),
                    &owed.to,
                    Some(anyhow::anyhow!("the session went before it took the turn")),
                );
                return None;
            }
        };
        loop {
            if !self.sent_work.lock().held.turns.contains(owed) {
                return None;
            }
            let lookup = project.clone();
            let now = self
                .store_blocking(move |store| {
                    if store.get_project(&lookup)?.is_none() {
                        return Ok(None);
                    }
                    let (_, worktrees, agents, _) = store.load_tree()?;
                    Ok(Some(crate::orchestrator::project_captain(
                        &worktrees, &agents, &lookup,
                    )))
                })
                .await;
            match now {
                Ok(Some(Some(captain))) => {
                    return self.sent_work.lock().held.retarget(owed, captain, self);
                }
                Ok(None) => {
                    self.drop_held(owed, "a held merge note went with its project");
                    return None;
                }
                Ok(Some(None)) | Err(_) => {
                    tokio::time::sleep(crate::session_context::DONE_POLL).await
                }
            }
        }
    }

    fn drop_held(&self, owed: &Owed, why: &str) {
        let mut work = self.sent_work.lock();
        if work.held.turns.contains(owed) {
            work.held.drop_turn(owed, self);
            tracing::warn!(to = %owed.to, "{why}");
        }
    }

    /// `ClientRequest::MergeNote`: hold `text`, the MERGE NOTE for the pull
    /// request at `pr_url`, for the CAPTAIN of `project`. Refused when the
    /// project has none.
    pub async fn merge_note(
        self: &Arc<Self>,
        project: &ProjectId,
        pr_url: &str,
        text: &str,
    ) -> Result<()> {
        let text = crate::orchestrator::validate_message(text)?.to_string();
        let lookup = project.clone();
        let captain = self
            .store_blocking(move |store| {
                let (_, worktrees, agents, _) = store.load_tree()?;
                Ok(crate::orchestrator::project_captain(
                    &worktrees, &agents, &lookup,
                ))
            })
            .await?
            .with_context(|| format!("project {project} has no captain to tell of the merge"))?;
        let owed = Owed {
            to: captain,
            at: nebula_core::clock::now_ms(),
            about: About::MergeNote {
                pr_url: pr_url.to_string(),
                project: project.clone(),
            },
            text,
        };
        self.hold(owed, None);
        Ok(())
    }

    /// The turns held when the last daemon stopped, kept in `file` from now
    /// on: a merge note held for the captain its project has now, each
    /// one's captain booted when it has no live process, as a Start boots
    /// it, and the turn delivered as any held one is.
    pub fn restore_held(self: &Arc<Self>, file: PathBuf) {
        let held = read(&file);
        self.sent_work.lock().held = Held {
            turns: held.clone(),
            file: Some(file),
            rank_typed: None,
        };
        for mut owed in held {
            if let About::MergeNote { project, .. } = &owed.about {
                let now = self
                    .store
                    .load_tree()
                    .ok()
                    .and_then(|(_, worktrees, agents, _)| {
                        crate::orchestrator::project_captain(&worktrees, &agents, project)
                    });
                if let Some(captain) = now {
                    let moved = self.sent_work.lock().held.retarget(&owed, captain, self);
                    owed = moved.unwrap_or(owed);
                }
            }
            let sref = SessionRef::Agent(owed.to.clone());
            if let Err(err) =
                self.ensure_session(&sref, crate::pty::DEFAULT_COLS, crate::pty::DEFAULT_ROWS)
            {
                tracing::warn!(to = %owed.to, "held turn's session not booted: {err:#}");
            }
            let daemon = self.clone();
            tokio::spawn(async move { daemon.deliver(owed, None).await });
        }
    }

    /// What every subscriber is told of the held briefs, as a new one is
    /// after its Snapshot.
    pub fn held_briefs(&self) -> ServerEvent {
        self.sent_work.lock().held.event()
    }

    /// What every subscriber is told of the RANK TURN, as a new one is
    /// after its Snapshot.
    pub fn rank_turn(&self) -> ServerEvent {
        self.sent_work.lock().held.rank_event()
    }

    /// `ClientRequest::RankFocus`: hold the RANK TURN for the ORCHESTRATOR,
    /// booting it when it has no live process, and return the orchestrator.
    /// Without `wait`, refused unless the orchestrator is ready for a turn
    /// now ([`Self::ready_for_turn`]) and owed no other held turn, so a
    /// rank the page asks for on its own never waits to land in the middle
    /// of a conversation. A rank turn held already is joined.
    pub async fn rank_focus(self: &Arc<Self>, wait: bool) -> Result<AgentId> {
        let orchestrator = self
            .store_blocking(|store| {
                Ok(store
                    .load_tree()?
                    .2
                    .into_iter()
                    .find(|a| a.role == AgentRole::Orchestrator && !a.archived))
            })
            .await?
            .context("there is no orchestrator to rank the picks: F10 in the TUI starts one")?;
        let id = orchestrator.id;
        if wait {
            self.ensure_session(
                &SessionRef::Agent(id.clone()),
                crate::pty::DEFAULT_COLS,
                crate::pty::DEFAULT_ROWS,
            )?;
        } else {
            let daemon = self.clone();
            let look = id.clone();
            let ready = tokio::task::spawn_blocking(move || {
                daemon.ready_for_turn(&look, AgentRole::Orchestrator)
            })
            .await??
            .is_some();
            let owed = self.sent_work.lock().held.turns.iter().any(|o| o.to == id);
            if !ready || owed {
                bail!("the orchestrator is not at rest with an empty input box");
            }
        }
        let owed = Owed {
            to: id.clone(),
            at: nebula_core::clock::now_ms(),
            about: About::Rank,
            text: RANK_TURN.to_string(),
        };
        self.hold(owed, None);
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merge_note(n: u32) -> About {
        About::MergeNote {
            pr_url: format!("https://github.com/o/r/pull/{n}"),
            project: ProjectId("p".into()),
        }
    }

    fn owed(to: &str, about: About) -> Owed {
        Owed {
            to: AgentId(to.into()),
            at: 5,
            about,
            text: "the turn".into(),
        }
    }

    /// The file round-trips, and one that does not read holds nothing.
    #[test]
    fn held_turns_outlast_the_daemon_in_their_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("held-turns.json");
        assert!(read(&file).is_empty(), "no file holds nothing");
        let held = vec![
            owed(
                "c",
                About::Brief("https://linear.app/acme/issue/ACME-7".into()),
            ),
            owed("c", merge_note(7)),
        ];
        write(&file, &held).unwrap();
        assert_eq!(read(&file), held);
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(
            json[0]["about"]["brief"],
            "https://linear.app/acme/issue/ACME-7"
        );
        assert_eq!(
            json[1]["about"]["merge_note"]["pr_url"],
            "https://github.com/o/r/pull/7"
        );
        assert_eq!(json[1]["about"]["merge_note"]["project"], "p");
        std::fs::write(&file, "not json").unwrap();
        assert!(read(&file).is_empty());
    }

    /// Each captain takes its turns oldest first, whatever they are, and
    /// subscribers are told only of the briefs; a merge note handed over
    /// twice is held once.
    #[tokio::test]
    async fn a_captains_held_turns_go_oldest_first() {
        let daemon = Daemon::new(
            Arc::new(crate::store::Store::open_in_memory().unwrap()),
            crate::hooks::HookEnv {
                port: 0,
                token: String::new(),
            },
        );
        let note = owed("a", merge_note(7));
        let brief = owed(
            "a",
            About::Brief("https://linear.app/acme/issue/ACME-7".into()),
        );
        let other = owed(
            "b",
            About::Brief("https://linear.app/acme/issue/ACME-8".into()),
        );
        for turn in [&note, &brief, &note, &other] {
            daemon.hold(turn.clone(), None);
        }
        let mut work = daemon.sent_work.lock();
        assert_eq!(
            work.held.turns,
            [note.clone(), brief.clone(), other.clone()]
        );
        assert_eq!(work.held.next(&note), Some(true));
        assert_eq!(work.held.next(&brief), Some(false), "after the note");
        assert_eq!(work.held.next(&other), Some(true), "another captain's");
        let ServerEvent::HeldBriefs { briefs } = work.held.event() else {
            unreachable!()
        };
        let told: Vec<&str> = briefs.iter().map(|b| b.issue_url.as_str()).collect();
        assert_eq!(
            told,
            [
                "https://linear.app/acme/issue/ACME-7",
                "https://linear.app/acme/issue/ACME-8"
            ]
        );
        assert_eq!(Held::default().next(&note), None, "held no more");
        // A merge note moved on to another captain keeps its place in line.
        let moved = work
            .held
            .retarget(&note, AgentId("b".into()), &daemon)
            .unwrap();
        assert_eq!(moved.to, AgentId("b".into()));
        assert_eq!(work.held.next(&moved), Some(true), "ahead of b's brief");
        assert_eq!(work.held.next(&other), Some(false));
        assert_eq!(work.held.next(&brief), Some(true), "a's next now");
        assert_eq!(work.held.next(&note), None, "held for a no more");
    }

    /// A same-session schedule's turn waits for its session as a worker's,
    /// one at a time for each schedule: a run while one is held joins it,
    /// and says so to the run log by being refused here.
    #[tokio::test]
    async fn a_schedules_turn_is_held_once_for_its_session() {
        let daemon = Daemon::new(
            Arc::new(crate::store::Store::open_in_memory().unwrap()),
            crate::hooks::HookEnv {
                port: 0,
                token: String::new(),
            },
        );
        let turn = |schedule: &str, fresh| {
            owed(
                "w",
                About::Schedule {
                    schedule: schedule.into(),
                    fresh,
                },
            )
        };
        assert_eq!(turn("a", false).about.role(), AgentRole::Worker);
        assert!(!daemon.schedule_held("a"));
        assert!(daemon.hold(turn("a", false), None));
        assert!(
            !daemon.hold(turn("a", true), None),
            "the second run joins it"
        );
        assert!(daemon.hold(turn("b", true), None), "another schedule's");
        assert!(daemon.schedule_held("a") && daemon.schedule_held("b"));
        let work = daemon.sent_work.lock();
        assert_eq!(work.held.turns, [turn("a", false), turn("b", true)]);
        assert_eq!(work.held.next(&turn("a", false)), Some(true));
        assert_eq!(
            work.held.next(&turn("b", true)),
            Some(false),
            "one at a time"
        );
        let ServerEvent::HeldBriefs { briefs } = work.held.event() else {
            unreachable!()
        };
        assert!(briefs.is_empty(), "no brief");
        let json = serde_json::to_value(turn("b", true)).unwrap();
        assert_eq!(json["about"]["schedule"]["schedule"], "b");
        assert_eq!(json["about"]["schedule"]["fresh"], true);
    }

    /// One rank turn is held at a time, for the orchestrator, and
    /// subscribers are told when it was held and, once it is typed, when.
    #[tokio::test]
    async fn one_rank_turn_is_held_for_the_orchestrator() {
        let daemon = Daemon::new(
            Arc::new(crate::store::Store::open_in_memory().unwrap()),
            crate::hooks::HookEnv {
                port: 0,
                token: String::new(),
            },
        );
        let rank = |at| Owed {
            at,
            ..owed("o", About::Rank)
        };
        assert_eq!(About::Rank.role(), AgentRole::Orchestrator);
        assert_eq!(merge_note(1).role(), AgentRole::Captain);
        daemon.hold(rank(5), None);
        daemon.hold(rank(6), None);
        let turn = |work: &crate::orchestrator::SentWork| match work.held.rank_event() {
            ServerEvent::RankTurn { turn } => turn,
            _ => unreachable!(),
        };
        let mut work = daemon.sent_work.lock();
        assert_eq!(work.held.turns, [rank(5)], "the second joined the first");
        assert_eq!(
            turn(&work),
            RankTurn {
                held_at: Some(5),
                typed_at: None
            }
        );
        let before = nebula_core::clock::now_ms();
        work.held.typing(&rank(5), &daemon);
        let typed = turn(&work);
        assert_eq!(typed.held_at, None);
        assert!(typed.typed_at.is_some_and(|at| at >= before), "{typed:?}");
        let json = serde_json::to_value(rank(5)).unwrap();
        assert_eq!(json["about"], "rank");
        assert_eq!(json["captain"], "o", "the file keeps its old field name");
    }
}
