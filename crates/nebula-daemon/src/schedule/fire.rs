//! A SCHEDULE's run: a new session on its prompt, in a new worktree or the
//! root checkout; its prompt held for its one session, typed or its CLI
//! started again on a new conversation once that is at rest with an empty
//! input box (a HELD TURN, `held`); or the RANK TURN, held for the
//! orchestrator once `nebula serve`, which holds the candidates it ranks,
//! is up.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use nebula_core::{
    paths, AgentId, AgentRole, EntityId, RunOutcome, Schedule, ScheduleRun, ScheduleTarget,
    ScheduleTask, SessionRef,
};

use crate::held::{About, Owed};
use crate::registry::{CreateAgentSpec, Daemon};

/// What a same-session run says while its last run's turn still waits.
const STILL_HELD: &str = "The last run still waits for its session to be at rest with an empty \
input box, so this one was not held beside it";

impl Daemon {
    /// Run `schedule`, due at `due` (none: asked for now), with `note`, and
    /// keep the run in the log: what it did, as it stands once it has done
    /// what it does at once.
    pub(crate) async fn fire(
        self: &Arc<Self>,
        schedule: &Schedule,
        due: Option<i64>,
        note: Option<String>,
    ) -> ScheduleRun {
        let run = |outcome, note: Option<String>| ScheduleRun {
            schedule: schedule.id.clone(),
            due_at: due,
            at: nebula_core::clock::now_ms(),
            outcome,
            note,
        };
        let went = match &schedule.spec.task {
            ScheduleTask::Rank => self.fire_rank().await,
            ScheduleTask::Prompt { .. } => self.fire_prompt(schedule).await,
        };
        let (outcome, said) = went.unwrap_or_else(|err| {
            (
                RunOutcome::Failed {
                    why: format!("{err:#}"),
                },
                None,
            )
        });
        let note = match (note, said) {
            (Some(note), Some(said)) => Some(format!("{note}. {said}")),
            (note, said) => note.or(said),
        };
        let done = run(outcome, note);
        tracing::info!(schedule = %schedule.id, outcome = ?done.outcome, "schedule ran");
        self.record_run(done.clone());
        done
    }

    /// The RANK TURN, held for the ORCHESTRATOR, once a `nebula serve` is
    /// up for it to read the candidates from: started as `⇧W` starts one,
    /// when none answers.
    async fn fire_rank(self: &Arc<Self>) -> Result<(RunOutcome, Option<String>)> {
        tokio::task::spawn_blocking(serve_up)
            .await
            .context("the serve's start panicked")?
            .map_err(anyhow::Error::msg)?;
        let orchestrator = self.rank_focus(true).await?;
        Ok((
            RunOutcome::Held {
                session: orchestrator,
            },
            None,
        ))
    }

    /// The prompt, where its schedule sends it.
    async fn fire_prompt(
        self: &Arc<Self>,
        schedule: &Schedule,
    ) -> Result<(RunOutcome, Option<String>)> {
        let ScheduleTask::Prompt {
            project,
            prompt,
            kind,
            custom_harness,
            model,
            effort,
            permission_mode,
            target,
        } = &schedule.spec.task
        else {
            unreachable!("a rank is no prompt");
        };
        let spec = |worktree| CreateAgentSpec {
            worktree,
            name: schedule.spec.name.clone(),
            kind: *kind,
            custom_harness: custom_harness.clone(),
            model: model.clone(),
            effort: effort.clone(),
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: Some(prompt.clone()),
            pr_url: None,
            issue_url: None,
            role: AgentRole::Worker,
            fork_of: None,
            permission_mode: permission_mode.clone(),
        };
        let lookup = project.clone();
        let (root, branches) = self
            .store_blocking(move |store| {
                let (_, worktrees, _, _) = store.load_tree()?;
                let root = worktrees
                    .iter()
                    .find(|w| w.project_id == lookup && w.is_main)
                    .cloned();
                let branches: Vec<String> = worktrees
                    .into_iter()
                    .filter(|w| w.project_id == lookup)
                    .map(|w| w.branch)
                    .collect();
                Ok((root, branches))
            })
            .await?;
        let root = root.context("its project is gone from nebula")?;
        match target {
            ScheduleTarget::NewWorktree => {
                // Whatever the create would refuse is asked before git cuts
                // a checkout nobody would then launch in.
                self.check_cold_launch(*kind, custom_harness.as_deref(), prompt)
                    .await?;
                let branch =
                    run_branch(&schedule.spec.name, nebula_core::clock::now_ms(), &branches);
                let worktree = self
                    .worktree_on_branch_for_spawn(project, &branch, None)
                    .await?;
                let session = agent_of(self.create_agent(spec(worktree.id)).await?);
                Ok((RunOutcome::Started { session }, None))
            }
            ScheduleTarget::Root => {
                let session = agent_of(self.create_agent(spec(root.id)).await?);
                Ok((RunOutcome::Started { session }, None))
            }
            ScheduleTarget::Session | ScheduleTarget::FreshSession => {
                let _one = self.schedule_op.lock().await;
                let had = self
                    .schedules
                    .lock()
                    .get(&schedule.id)
                    .and_then(|s| s.session.clone());
                let live = match had.clone() {
                    Some(id) => self
                        .store_blocking(move |store| store.get_agent(&id))
                        .await?
                        .filter(|a| !a.archived)
                        .map(|a| a.id),
                    None => None,
                };
                if let Some(session) = live {
                    if self.schedule_held(&schedule.id) {
                        return Ok((
                            RunOutcome::Skipped {
                                why: STILL_HELD.into(),
                            },
                            None,
                        ));
                    }
                    // A session the idle reaper took comes back first, on its
                    // conversation: the turn waits for it to be up.
                    self.ensure_session(
                        &SessionRef::Agent(session.clone()),
                        crate::pty::DEFAULT_COLS,
                        crate::pty::DEFAULT_ROWS,
                    )?;
                    let owed = Owed {
                        to: session.clone(),
                        at: nebula_core::clock::now_ms(),
                        about: About::Schedule {
                            schedule: schedule.id.clone(),
                            fresh: *target == ScheduleTarget::FreshSession,
                        },
                        text: prompt.clone(),
                    };
                    // The run says held before the turn can go, which is a
                    // beat of `when_ready` at the soonest.
                    if !self.hold(owed, None) {
                        return Ok((
                            RunOutcome::Skipped {
                                why: STILL_HELD.into(),
                            },
                            None,
                        ));
                    }
                    return Ok((RunOutcome::Held { session }, None));
                }
                let session = agent_of(self.create_agent(spec(root.id)).await?);
                {
                    let mut book = self.schedules.lock();
                    if let Some(kept) = book.kept.schedules.iter_mut().find(|s| s.id == schedule.id)
                    {
                        kept.session = Some(session.clone());
                    }
                }
                let said = had.map(|_| {
                    "Its session was archived or deleted, so this run started another".to_string()
                });
                Ok((RunOutcome::Started { session }, said))
            }
        }
    }
}

fn agent_of(created: EntityId) -> AgentId {
    match created {
        EntityId::Agent(id) => id,
        _ => unreachable!("create_agent returns an agent id"),
    }
}

/// The branch a run's new worktree is cut on: the schedule's name and when
/// it ran, `nightly-triage-1116-0730`, one none of `taken` has.
fn run_branch(name: &str, at: i64, taken: &[String]) -> String {
    let mut slug = String::new();
    for word in name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        if slug.len() + word.len() > 32 {
            break;
        }
        if !slug.is_empty() {
            slug.push('-');
        }
        slug.push_str(&word.to_ascii_lowercase());
    }
    if slug.is_empty() {
        slug.push_str("schedule");
    }
    let c = super::when::civil(at, &super::when::Local);
    let base = format!(
        "{slug}-{:02}{:02}-{:02}{:02}",
        c.month, c.day, c.hour, c.minute
    );
    (1..)
        .map(|n| match n {
            1 => base.clone(),
            n => format!("{base}-{n}"),
        })
        .find(|b| !taken.contains(b))
        .expect("an unbounded counter always finds a free name")
}

/// The `nebula serve` this daemon's page is served by, started as `⇧W`
/// starts one when none answers: through the user's login shell, so it
/// reads Linear and GitHub with the keys and `PATH` a terminal has, even
/// in a daemon a login item started. Its base URL, or why not.
fn serve_up() -> Result<String, String> {
    let log = paths::log_dir().join("serve.log");
    let start = || {
        let exe = paths::current_exe()?;
        let line = format!(
            "exec {} serve --no-open",
            nebula_core::shell::single_quote(&exe.to_string_lossy())
        );
        let shell = nebula_core::shell::user_shell();
        let args = ["-l".to_string(), "-i".into(), "-c".into(), line];
        nebula_client::serve::start_serve(Path::new(&shell), &args, &log)
    };
    nebula_client::serve::serve_url(
        &paths::serve_port_path(),
        start,
        nebula_client::serve::START_WAIT,
    )
    .map_err(|why| format!("{why}: see {}", log.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_runs_worktree_is_named_for_its_schedule_and_when_it_ran() {
        let at =
            super::super::when::at_civil(2026, 11, 16, 7, 30, &super::super::when::Local).unwrap();
        assert_eq!(
            run_branch("Nightly triage: CI", at, &[]),
            "nightly-triage-ci-1116-0730"
        );
        let taken = ["nightly-triage-ci-1116-0730".to_string()];
        assert_eq!(
            run_branch("Nightly triage: CI", at, &taken),
            "nightly-triage-ci-1116-0730-2"
        );
        assert_eq!(run_branch("!!", at, &[]), "schedule-1116-0730");
    }
}
