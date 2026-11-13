//! HELD TURNS: the turns the daemon types into a CAPTAIN that nobody at its
//! keys typed, each held until the captain is free to take it: a Start's
//! BRIEF (`start_issue`) and the MERGE NOTE a TUI hands over once it sees a
//! pull request merge (`ClientRequest::MergeNote`).
//!
//! None is typed into a captain at work, into a prompt of the user's, or in
//! place of the answer to a question the captain asked the user: a held
//! turn goes once the captain is at rest with its input box empty and its
//! last turn asking nothing (`Daemon::when_ready`, the wait a DONE NOTE
//! has), oldest first, and the next only once the captain has taken the
//! last. A brief is for the captain Start got or started, and goes with it;
//! a merge note is for its project, and goes to whichever captain the
//! project has when the note goes. Held turns are kept in `held-turns.json`
//! in the data dir, so a daemon restart delivers them too, and every
//! subscriber is told which briefs are held (`ServerEvent::HeldBriefs`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use nebula_core::{AgentId, AgentRole, HeldBrief, ProjectId, ServerEvent, SessionRef};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::registry::Daemon;

/// The turns held, and the file they are kept in.
#[derive(Default)]
pub(crate) struct Held {
    turns: Vec<Owed>,
    /// None until the daemon restores them as it boots, and in tests, where
    /// nothing is written.
    file: Option<PathBuf>,
}

/// A held turn: the text typed into `captain` once it is free.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Owed {
    /// The captain it waits for: a merge note's moves on to its project's
    /// next captain should this one go ([`Daemon::redirect`]).
    pub(crate) captain: AgentId,
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
                    captain: o.captain.clone(),
                    at: o.at,
                }),
                About::MergeNote { .. } => None,
            })
            .collect();
        ServerEvent::HeldBriefs { briefs }
    }

    /// Whether `owed` is next for its captain, the oldest held for it;
    /// `None` once it is held no more.
    fn next(&self, owed: &Owed) -> Option<bool> {
        self.turns
            .contains(owed)
            .then(|| self.turns.iter().find(|o| o.captain == owed.captain) == Some(owed))
    }

    fn drop_turn(&mut self, owed: &Owed, daemon: &Daemon) {
        self.turns.retain(|o| o != owed);
        self.changed(daemon);
    }

    /// `owed`, held for `captain` from now on, in its place among the
    /// others; `None` once it is held no more.
    fn retarget(&mut self, owed: &Owed, captain: AgentId, daemon: &Daemon) -> Option<Owed> {
        let held = self.turns.iter_mut().find(|o| *o == owed)?;
        if held.captain == captain {
            return Some(held.clone());
        }
        held.captain = captain;
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
    /// Hold `owed` until its captain takes it ([`Self::deliver`]), and tell
    /// `taken` when it does. A merge note for a pull request one is held
    /// for already is not held again: two TUIs can see one merge.
    pub(crate) fn hold(self: &Arc<Self>, owed: Owed, taken: Option<oneshot::Sender<()>>) {
        {
            let mut work = self.sent_work.lock();
            let held = &mut work.held;
            if matches!(owed.about, About::MergeNote { .. })
                && held.turns.iter().any(|o| o.about == owed.about)
            {
                return;
            }
            held.turns.push(owed.clone());
            held.changed(self);
        }
        let daemon = self.clone();
        tokio::spawn(async move { daemon.deliver(owed, taken).await });
    }

    /// Type `owed` into its captain once that is ready for a turn and
    /// `owed` is the oldest it is owed ([`Self::when_ready`]), then tell
    /// `taken`. A captain archived or deleted before that takes a brief
    /// with it, and hands a merge note on ([`Self::redirect`]).
    async fn deliver(self: &Arc<Self>, mut owed: Owed, taken: Option<oneshot::Sender<()>>) {
        let session = loop {
            let ready = self
                .when_ready(
                    &owed.captain,
                    AgentRole::Captain,
                    |work| work.held.next(&owed),
                    |work| work.held.drop_turn(&owed, self),
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
        if let Err(err) = self.type_turn(&owed.captain, &session, &owed.text).await {
            tracing::warn!(captain = %owed.captain, "held turn typed, not taken whole: {err:#}");
        }
    }

    /// Where `owed` goes once [`Self::when_ready`] gave up on its captain: a
    /// brief nowhere, as it goes with its captain; a merge note to the
    /// captain its project has now, as `captain` finds it, waiting while the
    /// project has none. `None` once it is held no more, or goes nowhere,
    /// which is said in the log: a brief, and a merge note whose project is
    /// gone.
    async fn redirect(self: &Arc<Self>, owed: &Owed) -> Option<Owed> {
        let About::MergeNote { project, .. } = &owed.about else {
            self.drop_held(owed, "a held brief went with its captain");
            return None;
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
            tracing::warn!(captain = %owed.captain, "{why}");
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
            captain,
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
            let sref = SessionRef::Agent(owed.captain.clone());
            if let Err(err) =
                self.ensure_session(&sref, crate::pty::DEFAULT_COLS, crate::pty::DEFAULT_ROWS)
            {
                tracing::warn!(captain = %owed.captain, "held turn's captain not booted: {err:#}");
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

    fn owed(captain: &str, about: About) -> Owed {
        Owed {
            captain: AgentId(captain.into()),
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
                About::Brief("https://linear.app/acme/issue/REL-7".into()),
            ),
            owed("c", merge_note(7)),
        ];
        write(&file, &held).unwrap();
        assert_eq!(read(&file), held);
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(
            json[0]["about"]["brief"],
            "https://linear.app/acme/issue/REL-7"
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
            About::Brief("https://linear.app/acme/issue/REL-7".into()),
        );
        let other = owed(
            "b",
            About::Brief("https://linear.app/acme/issue/REL-8".into()),
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
                "https://linear.app/acme/issue/REL-7",
                "https://linear.app/acme/issue/REL-8"
            ]
        );
        assert_eq!(Held::default().next(&note), None, "held no more");
        // A merge note moved on to another captain keeps its place in line.
        let moved = work
            .held
            .retarget(&note, AgentId("b".into()), &daemon)
            .unwrap();
        assert_eq!(moved.captain, AgentId("b".into()));
        assert_eq!(work.held.next(&moved), Some(true), "ahead of b's brief");
        assert_eq!(work.held.next(&other), Some(false));
        assert_eq!(work.held.next(&brief), Some(true), "a's next now");
        assert_eq!(work.held.next(&note), None, "held for a no more");
    }
}
