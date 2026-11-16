//! SCHEDULES: a prompt in a project, or the orchestrator's RANK TURN, run
//! on a form of time by the daemon, with no launchd job and no page open.
//!
//! A beat each second compares every schedule's next due time with the
//! wall clock (`nebula_core::clock::now_ms`), so a Mac that slept through a
//! due time finds it due on the first beat after it wakes, and the rules
//! for a late run decide what then runs ([`when::decide`]). Every run is
//! kept in the RUN LOG with what it did, and every run skipped with why.
//!
//! Kept in `schedules.json` in the data dir, written renamed into place,
//! apart from the store's SQL migrations, whose numbers are positional and
//! collide with upstream's. The file also keeps the permission mode each
//! session a schedule started launches with, since that is no column of
//! its row: every start of its CLI, a resume included, reads it there.
//! Every subscriber is told the schedules and the run log right after its
//! Snapshot and on every change (`ServerEvent::Schedules`).

pub mod when;

mod fire;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use nebula_core::{
    AgentId, AgentKind, RunOutcome, Schedule, ScheduleBook, ScheduleRun, ScheduleSpec,
    ScheduleTask, ScheduleWhen, ServerEvent, DAYS_ALL, PERMISSION_MODES,
};
use serde::{Deserialize, Serialize};

use crate::registry::Daemon;

/// How many runs each schedule's log keeps, newest last.
pub const RUNS_KEPT: usize = 50;
/// How often the due times are looked at.
const BEAT: Duration = Duration::from_secs(1);
/// The longest a schedule's name may be.
const NAME_MAX: usize = 60;
/// The longest a schedule's interval may be: four weeks.
const EVERY_MAX: u32 = 4 * 7 * 24 * 60;

/// What `schedules.json` holds.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
struct Kept {
    #[serde(default)]
    schedules: Vec<Schedule>,
    #[serde(default)]
    runs: Vec<ScheduleRun>,
    /// The permission mode each session a schedule started launches with.
    #[serde(default)]
    modes: BTreeMap<AgentId, String>,
}

/// The schedules, their run log, and the file they are kept in.
#[derive(Default)]
pub(crate) struct Book {
    kept: Kept,
    /// None until the daemon restores them as it boots, and in tests,
    /// where nothing is written.
    file: Option<PathBuf>,
}

impl Book {
    /// What every subscriber is told of them.
    pub(crate) fn event(&self) -> ServerEvent {
        ServerEvent::Schedules {
            book: ScheduleBook {
                schedules: self.kept.schedules.clone(),
                runs: self.kept.runs.clone(),
            },
        }
    }

    /// Write the file again, renamed into place, and tell every subscriber,
    /// both under the lock the change was made in.
    fn changed(&self, daemon: &Daemon) {
        if let Some(file) = &self.file {
            if let Err(err) = write(file, &self.kept) {
                tracing::warn!(file = %file.display(), "schedules not written: {err:#}");
            }
        }
        daemon.broadcast(self.event());
    }

    /// The permission mode `agent` launches with, when a schedule started it
    /// with one.
    pub(crate) fn mode(&self, agent: &AgentId) -> Option<String> {
        self.kept.modes.get(agent).cloned()
    }

    pub(crate) fn set_mode(&mut self, agent: &AgentId, mode: &str, daemon: &Daemon) {
        self.kept.modes.insert(agent.clone(), mode.to_string());
        self.changed(daemon);
    }

    fn get(&self, id: &str) -> Option<&Schedule> {
        self.kept.schedules.iter().find(|s| s.id == id)
    }

    /// Keep `run` in the log, the schedule's oldest going once it has more
    /// than [`RUNS_KEPT`].
    fn record(&mut self, run: ScheduleRun) {
        let schedule = run.schedule.clone();
        self.kept.runs.push(run);
        let of = |r: &ScheduleRun| r.schedule == schedule;
        let mut over = self.kept.runs.iter().filter(|r| of(r)).count();
        self.kept.runs.retain(|r| {
            let drop = over > RUNS_KEPT && of(r);
            over -= usize::from(drop);
            !drop
        });
    }

    /// The runs due at `now`, each with when it was due and its note, with
    /// every schedule's next due time moved on and every run skipped kept.
    fn due(&mut self, now: i64, zone: &impl when::Zone) -> Vec<(Schedule, i64, Option<String>)> {
        let mut due = Vec::new();
        let mut skipped = Vec::new();
        for schedule in &mut self.kept.schedules {
            let Some(at) = schedule.next_at.filter(|&at| at <= now) else {
                continue;
            };
            let decided = when::decide(&schedule.spec.when, at, now, zone);
            schedule.next_at = decided.next;
            skipped.extend(decided.skipped.into_iter().map(|(at, why)| ScheduleRun {
                schedule: schedule.id.clone(),
                due_at: Some(at),
                at: now,
                outcome: RunOutcome::Skipped { why },
                note: None,
            }));
            if let Some((at, note)) = decided.run {
                due.push((schedule.clone(), at, note));
            }
        }
        for run in skipped {
            self.record(run);
        }
        due
    }
}

fn write(file: &Path, kept: &Kept) -> Result<()> {
    let tmp = file.with_extension(format!("json.{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(kept)?)?;
    std::fs::rename(&tmp, file)?;
    Ok(())
}

/// What `file` keeps: nothing when there is no file, and nothing, said
/// once, when it does not read.
fn read(file: &Path) -> Kept {
    match std::fs::read(file) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
            tracing::warn!(file = %file.display(), "schedules not read: {err}");
            Kept::default()
        }),
        Err(_) => Kept::default(),
    }
}

/// The wall clock the schedules read: the time, plus the skew a test's
/// [`nebula_core::env::SCHEDULE_SKEW_FILE`] holds when that is set.
fn now() -> i64 {
    let skew = std::env::var_os(nebula_core::env::SCHEDULE_SKEW_FILE)
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|text| text.trim().parse::<i64>().ok())
        .unwrap_or(0);
    nebula_core::clock::now_ms() + skew
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A name for a schedule given none: its prompt's first words, or the
/// rank's.
fn name_of(spec: &ScheduleSpec) -> String {
    let named = one_line(&spec.name);
    if !named.is_empty() {
        return named;
    }
    match &spec.task {
        ScheduleTask::Rank => "Morning rank".into(),
        ScheduleTask::Prompt { prompt, .. } => {
            let line = one_line(prompt);
            let mut name = String::new();
            for word in line.split(' ') {
                if name.len() + word.len() + 1 > 40 {
                    break;
                }
                if !name.is_empty() {
                    name.push(' ');
                }
                name.push_str(word);
            }
            if name.is_empty() {
                line.chars().take(40).collect()
            } else {
                name
            }
        }
    }
}

impl Daemon {
    /// The schedules and run log the last daemon kept in `file`, kept there
    /// from now on.
    pub fn restore_schedules(self: &Arc<Self>, file: PathBuf) {
        let mut kept = read(&file);
        // A session deleted since takes its permission mode with it.
        kept.modes
            .retain(|id, _| self.store.get_agent(id).ok().flatten().is_some());
        let mut book = self.schedules.lock();
        *book = Book {
            kept,
            file: Some(file),
        };
        book.changed(self);
    }

    /// What every subscriber is told of the schedules, as a new one is
    /// after its Snapshot.
    pub fn schedules_event(&self) -> ServerEvent {
        self.schedules.lock().event()
    }

    /// Run every schedule found due on each beat, until the daemon stops.
    pub async fn run_schedules(self: &Arc<Self>) {
        let mut beat = tokio::time::interval(BEAT);
        beat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = self.shutdown.cancelled() => break,
                _ = beat.tick() => {}
            }
            let due = {
                let mut book = self.schedules.lock();
                let before = book.kept.schedules.clone();
                let due = book.due(now(), &when::Local);
                if book.kept.schedules != before {
                    book.changed(self);
                }
                due
            };
            for (schedule, at, note) in due {
                let daemon = self.clone();
                tokio::spawn(async move { daemon.fire(&schedule, Some(at), note).await });
            }
        }
    }

    /// `ClientRequest::AddSchedule`: the new schedule's id, once `spec` is
    /// one the daemon can run.
    pub async fn add_schedule(self: &Arc<Self>, spec: ScheduleSpec) -> Result<String> {
        let spec = self.checked_spec(spec).await?;
        let created_at = now();
        let next_at = match spec.when {
            ScheduleWhen::Once { at } if at <= created_at => {
                bail!("that time has passed: a once schedule runs at a time still to come")
            }
            when => when::due_after(&when, created_at, &when::Local),
        };
        let id = ulid::Ulid::generate().to_string().to_ascii_lowercase();
        let schedule = Schedule {
            id: id.clone(),
            spec,
            created_at,
            next_at,
            session: None,
        };
        let mut book = self.schedules.lock();
        book.kept.schedules.push(schedule);
        book.changed(self);
        Ok(id)
    }

    /// `ClientRequest::RemoveSchedule`: the schedule goes, its run log with
    /// it. A turn of it still held goes on to its session.
    pub fn remove_schedule(&self, id: &str) -> Result<()> {
        let mut book = self.schedules.lock();
        if book.get(id).is_none() {
            bail!("there is no schedule {id}");
        }
        book.kept.schedules.retain(|s| s.id != id);
        book.kept.runs.retain(|r| r.schedule != id);
        book.changed(self);
        Ok(())
    }

    /// `ClientRequest::RunSchedule`: run it now, leaving when it is due
    /// next as it was.
    pub async fn run_schedule_now(self: &Arc<Self>, id: &str) -> Result<ScheduleRun> {
        let schedule = self
            .schedules
            .lock()
            .get(id)
            .cloned()
            .with_context(|| format!("there is no schedule {id}"))?;
        Ok(self.fire(&schedule, None, None).await)
    }

    /// Keep `run` in the log, and tell every subscriber.
    pub(crate) fn record_run(&self, run: ScheduleRun) {
        let mut book = self.schedules.lock();
        if book.get(&run.schedule).is_some() {
            book.record(run);
            book.changed(self);
        }
    }

    /// A held turn of the schedule `schedule`, or with none the RANK TURN,
    /// went to `session`: typed, or with `failed` why not. The run that
    /// holds it says so from now on.
    pub(crate) fn schedule_turn_went(
        &self,
        schedule: Option<&str>,
        session: &AgentId,
        failed: Option<anyhow::Error>,
    ) {
        let mut book = self.schedules.lock();
        let ranks: Vec<String> = book
            .kept
            .schedules
            .iter()
            .filter(|s| s.spec.task == ScheduleTask::Rank)
            .map(|s| s.id.clone())
            .collect();
        let fresh: Vec<String> = book
            .kept
            .schedules
            .iter()
            .filter(|s| {
                matches!(
                    s.spec.task,
                    ScheduleTask::Prompt {
                        target: nebula_core::ScheduleTarget::FreshSession,
                        ..
                    }
                )
            })
            .map(|s| s.id.clone())
            .collect();
        let mut moved = false;
        for run in &mut book.kept.runs {
            let ours = match schedule {
                Some(id) => run.schedule == id,
                None => ranks.contains(&run.schedule),
            };
            if !ours || !matches!(&run.outcome, RunOutcome::Held { session: s } if s == session) {
                continue;
            }
            run.outcome = match &failed {
                Some(why) => RunOutcome::Failed {
                    why: format!("{why:#}"),
                },
                None if fresh.contains(&run.schedule) => RunOutcome::Restarted {
                    session: session.clone(),
                },
                None => RunOutcome::Typed {
                    session: session.clone(),
                },
            };
            moved = true;
        }
        if moved {
            book.changed(self);
        }
    }

    /// `spec` as it is kept, or why the daemon cannot run it: a name of its
    /// own, a form of time that can come round, and a prompt it can launch
    /// where it says, on a harness that can take each run there.
    async fn checked_spec(self: &Arc<Self>, mut spec: ScheduleSpec) -> Result<ScheduleSpec> {
        spec.name = name_of(&spec);
        if spec.name.chars().count() > NAME_MAX {
            bail!("a schedule's name is {NAME_MAX} characters at most");
        }
        match spec.when {
            ScheduleWhen::Every { minutes } if minutes == 0 || minutes > EVERY_MAX => {
                bail!("every N minutes takes 1 to {EVERY_MAX} minutes, four weeks")
            }
            ScheduleWhen::At { hour, minute, .. } if hour > 23 || minute > 59 => {
                bail!("a time of day is 00:00 to 23:59")
            }
            ScheduleWhen::At { days, .. } if days & DAYS_ALL == 0 => {
                bail!("a time of day runs on at least one day of the week")
            }
            _ => {}
        }
        if let ScheduleWhen::At { days, .. } = &mut spec.when {
            *days &= DAYS_ALL;
        }
        let ScheduleTask::Prompt {
            project,
            prompt,
            kind,
            custom_harness,
            model,
            effort,
            permission_mode,
            target,
        } = &mut spec.task
        else {
            return Ok(spec);
        };
        *prompt = crate::orchestrator::validate_message(prompt)?.to_string();
        for value in [
            &mut *model,
            &mut *effort,
            &mut *permission_mode,
            &mut *custom_harness,
        ] {
            *value = value
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("default"))
                .map(str::to_string);
        }
        let lookup = project.clone();
        let known = self
            .store_blocking(move |store| store.get_project(&lookup))
            .await?
            .with_context(|| format!("there is no project {project}"))?;
        if known.repo_path == nebula_core::paths::orchestrator_home() {
            bail!("the orchestrator's folder takes no schedules of its own");
        }
        let harness = crate::registry::resolve_harness(*kind, custom_harness.as_deref())?;
        refused_launch(&harness, *kind, permission_mode.as_deref(), *target)?;
        crate::registry::validate_starting_prompt(prompt)?;
        Ok(spec)
    }
}

/// Why a prompt on `harness` cannot run as `mode` and `target` ask, if it
/// cannot: a permission mode is Claude's flag alone, and the same session
/// takes its later runs only from a harness that says when it is at rest,
/// and that brings its conversation back after the idle reaper took it.
fn refused_launch(
    harness: &nebula_core::harness::HarnessDescriptor,
    kind: AgentKind,
    mode: Option<&str>,
    target: nebula_core::ScheduleTarget,
) -> Result<()> {
    let label = harness.display_label();
    if let Some(mode) = mode {
        if kind != AgentKind::Claude {
            bail!(
                "a permission mode is Claude Code's --permission-mode: {label} launches as nebula \
                 launches it, so leave the mode at default"
            );
        }
        if !PERMISSION_MODES.contains(&mode) {
            bail!(
                "Claude's permission modes are default, {}, not {mode}",
                PERMISSION_MODES.join(", ")
            );
        }
    }
    if !target.same_session() {
        return Ok(());
    }
    if harness.hook_dialect().is_none() {
        bail!(
            "{label} reports no status to nebula, so its session never reads as at rest and a \
             later run could never be typed into it: run each in a new session instead"
        );
    }
    if target == nebula_core::ScheduleTarget::Session && !harness.resumes() {
        bail!(
            "{label} cannot resume a conversation, so a session the idle reaper took would come \
             back without it: run each with fresh context, or in a new session"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::{ProjectId, ScheduleTarget};

    fn daemon() -> Arc<Daemon> {
        Daemon::new(
            Arc::new(crate::store::Store::open_in_memory().unwrap()),
            crate::hooks::HookEnv {
                port: 0,
                token: String::new(),
            },
        )
    }

    fn run(schedule: &str, at: i64) -> ScheduleRun {
        ScheduleRun {
            schedule: schedule.into(),
            due_at: Some(at),
            at,
            outcome: RunOutcome::Skipped { why: "w".into() },
            note: None,
        }
    }

    fn schedule(id: &str, when: ScheduleWhen, next_at: Option<i64>) -> Schedule {
        Schedule {
            id: id.into(),
            spec: ScheduleSpec {
                name: id.into(),
                task: ScheduleTask::Rank,
                when,
            },
            created_at: 0,
            next_at,
            session: None,
        }
    }

    /// Each schedule's log keeps its newest [`RUNS_KEPT`], whatever another's
    /// holds.
    #[test]
    fn the_run_log_keeps_each_schedules_newest_runs() {
        let mut book = Book::default();
        for at in 0..RUNS_KEPT as i64 + 7 {
            book.record(run("a", at));
        }
        book.record(run("b", 1));
        let of = |id: &str| -> Vec<i64> {
            book.kept
                .runs
                .iter()
                .filter(|r| r.schedule == id)
                .map(|r| r.at)
                .collect()
        };
        assert_eq!(of("a").len(), RUNS_KEPT);
        assert_eq!(of("a")[0], 7, "the oldest went");
        assert_eq!(*of("a").last().unwrap(), RUNS_KEPT as i64 + 6);
        assert_eq!(of("b"), [1]);
    }

    /// The file round-trips, and one that does not read keeps nothing.
    #[test]
    fn schedules_outlast_the_daemon_in_their_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("schedules.json");
        assert_eq!(read(&file), Kept::default());
        let kept = Kept {
            schedules: vec![schedule("a", ScheduleWhen::Every { minutes: 5 }, Some(9))],
            runs: vec![run("a", 3)],
            modes: BTreeMap::from([(AgentId("s".into()), "dontAsk".into())]),
        };
        write(&file, &kept).unwrap();
        assert_eq!(read(&file), kept);
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(json["schedules"][0]["spec"]["when"]["every"]["minutes"], 5);
        assert_eq!(json["modes"]["s"], "dontAsk");
        std::fs::write(&file, "not json").unwrap();
        assert_eq!(read(&file), Kept::default());
    }

    /// A beat runs what is due and moves its next time on, keeps the runs
    /// it skips, and leaves alone what is not due.
    #[test]
    fn a_beat_runs_what_is_due_and_keeps_what_it_skips() {
        struct Utc;
        impl when::Zone for Utc {
            fn offset(&self, _: i64) -> i64 {
                0
            }
        }
        let hour = 3_600_000;
        let weekdays = ScheduleWhen::At {
            hour: 9,
            minute: 0,
            days: nebula_core::DAYS_WEEKDAYS,
        };
        // Monday 2026-11-16 09:00 UTC.
        let monday = 1_794_819_600_000;
        let mut book = Book::default();
        book.kept.schedules = vec![
            schedule("every", ScheduleWhen::Every { minutes: 60 }, Some(monday)),
            schedule("weekdays", weekdays, Some(monday)),
            schedule(
                "later",
                ScheduleWhen::Every { minutes: 60 },
                Some(monday + 5 * hour),
            ),
        ];
        let due = book.due(monday + 2 * hour, &Utc);
        let ran: Vec<&str> = due.iter().map(|(s, _, _)| s.id.as_str()).collect();
        assert_eq!(ran, ["every"], "the weekday run two hours late is skipped");
        assert!(due[0]
            .2
            .as_deref()
            .unwrap()
            .starts_with("2 h late, once for the 3 runs"));
        assert_eq!(book.kept.schedules[0].next_at, Some(monday + 3 * hour));
        assert_eq!(book.kept.schedules[1].next_at, Some(monday + 24 * hour));
        assert_eq!(book.kept.schedules[2].next_at, Some(monday + 5 * hour));
        assert_eq!(book.kept.runs.len(), 1);
        assert_eq!(book.kept.runs[0].schedule, "weekdays");
        assert!(matches!(
            book.kept.runs[0].outcome,
            RunOutcome::Skipped { .. }
        ));
        assert!(
            book.due(monday + 2 * hour, &Utc).is_empty(),
            "nothing due twice"
        );
    }

    fn prompt(kind: AgentKind, mode: Option<&str>, target: ScheduleTarget) -> ScheduleSpec {
        ScheduleSpec {
            name: String::new(),
            task: ScheduleTask::Prompt {
                project: ProjectId("p".into()),
                prompt: "Triage the overnight failures".into(),
                kind,
                custom_harness: None,
                model: Some("default".into()),
                effort: None,
                permission_mode: mode.map(str::to_string),
                target,
            },
            when: ScheduleWhen::Every { minutes: 30 },
        }
    }

    /// What the daemon cannot run as asked is refused, saying why, before
    /// anything is kept.
    #[tokio::test]
    async fn a_schedule_it_cannot_run_as_asked_is_refused() {
        let daemon = daemon();
        daemon
            .store
            .insert_project(&nebula_core::Project {
                id: ProjectId("p".into()),
                name: "p".into(),
                repo_path: "/nebula-test/p".into(),
                sort_order: 0,
            })
            .unwrap();
        let refused = |spec| {
            let daemon = daemon.clone();
            async move { format!("{:#}", daemon.add_schedule(spec).await.unwrap_err()) }
        };
        let err = refused(prompt(
            AgentKind::Codex,
            Some("dontAsk"),
            ScheduleTarget::Root,
        ))
        .await;
        assert!(
            err.contains("a permission mode is Claude Code's --permission-mode"),
            "{err}"
        );
        let err = refused(prompt(
            AgentKind::Claude,
            Some("yolo"),
            ScheduleTarget::Root,
        ))
        .await;
        assert!(err.contains("not yolo"), "{err}");
        let err = refused(prompt(AgentKind::Muse, None, ScheduleTarget::FreshSession)).await;
        assert!(err.contains("reports no status to nebula"), "{err}");
        let err = refused(prompt(AgentKind::Grok, None, ScheduleTarget::Session)).await;
        assert!(err.contains("reports no status to nebula"), "{err}");
        let mut empty = prompt(AgentKind::Claude, None, ScheduleTarget::Root);
        let ScheduleTask::Prompt { prompt: text, .. } = &mut empty.task else {
            unreachable!()
        };
        *text = "  ".into();
        assert!(refused(empty).await.contains("empty"));
        let mut elsewhere = prompt(AgentKind::Claude, None, ScheduleTarget::Root);
        let ScheduleTask::Prompt { project, .. } = &mut elsewhere.task else {
            unreachable!()
        };
        *project = ProjectId("nope".into());
        assert!(refused(elsewhere)
            .await
            .contains("there is no project nope"));
        let mut never = prompt(AgentKind::Claude, None, ScheduleTarget::Root);
        never.when = ScheduleWhen::At {
            hour: 7,
            minute: 30,
            days: 0,
        };
        assert!(refused(never.clone()).await.contains("at least one day"));
        never.when = ScheduleWhen::Every { minutes: 0 };
        assert!(refused(never.clone()).await.contains("1 to"));
        never.when = ScheduleWhen::Once { at: 5 };
        assert!(refused(never).await.contains("that time has passed"));
        assert!(daemon.schedules.lock().kept.schedules.is_empty());

        // What it can run is kept with a name of its own, "default" as
        // none, and its first due time.
        let id = daemon
            .add_schedule(prompt(
                AgentKind::Claude,
                Some("acceptEdits"),
                ScheduleTarget::Session,
            ))
            .await
            .unwrap();
        let book = daemon.schedules.lock();
        let kept = book.get(&id).unwrap();
        assert_eq!(kept.spec.name, "Triage the overnight failures");
        let ScheduleTask::Prompt { model, .. } = &kept.spec.task else {
            unreachable!()
        };
        assert_eq!(model, &None);
        let due = kept.next_at.unwrap() - kept.created_at;
        assert_eq!(due, 30 * 60_000, "the first a period after it was added");
    }

    /// Codex says when it is at rest and resumes, so it takes either same
    /// session target; a harness that resumes nothing takes fresh context
    /// alone.
    #[test]
    fn a_harness_takes_the_same_session_only_as_far_as_it_can() {
        let registry = nebula_core::harness::registry(&BTreeMap::new(), &[]);
        let of = |id: &str| registry.iter().find(|h| h.id == id).unwrap().clone();
        for target in ScheduleTarget::ALL {
            assert!(refused_launch(&of("codex"), AgentKind::Codex, None, target).is_ok());
            assert!(refused_launch(&of("claude"), AgentKind::Claude, Some("plan"), target).is_ok());
        }
        let mut forgetful = of("codex");
        forgetful.resume = Default::default();
        assert!(refused_launch(
            &forgetful,
            AgentKind::Codex,
            None,
            ScheduleTarget::FreshSession
        )
        .is_ok());
        let err = refused_launch(&forgetful, AgentKind::Codex, None, ScheduleTarget::Session)
            .unwrap_err()
            .to_string();
        assert!(err.contains("cannot resume a conversation"), "{err}");
    }

    /// A held turn's run says what became of it once it went: typed,
    /// started again for fresh context, or why not.
    #[test]
    fn a_held_run_says_how_its_turn_went() {
        let daemon = daemon();
        let session = AgentId("s".into());
        let held = |schedule: &str| ScheduleRun {
            outcome: RunOutcome::Held {
                session: session.clone(),
            },
            ..run(schedule, 1)
        };
        {
            let mut book = daemon.schedules.lock();
            book.kept.schedules = vec![
                schedule("rank", ScheduleWhen::Every { minutes: 5 }, None),
                schedule("same", ScheduleWhen::Every { minutes: 5 }, None),
            ];
            let ScheduleTask::Prompt { .. } =
                &prompt(AgentKind::Claude, None, ScheduleTarget::Root).task
            else {
                unreachable!()
            };
            book.kept.schedules[1].spec =
                prompt(AgentKind::Claude, None, ScheduleTarget::FreshSession);
            book.record(held("rank"));
            book.record(held("same"));
        }
        daemon.schedule_turn_went(None, &session, None);
        daemon.schedule_turn_went(
            Some("same"),
            &session,
            Some(anyhow::anyhow!("the session went before it took the turn")),
        );
        let book = daemon.schedules.lock();
        assert_eq!(
            book.kept.runs[0].outcome,
            RunOutcome::Typed {
                session: session.clone()
            }
        );
        assert_eq!(
            book.kept.runs[1].outcome,
            RunOutcome::Failed {
                why: "the session went before it took the turn".into()
            }
        );
        drop(book);
        daemon.schedules.lock().kept.runs[1] = held("same");
        daemon.schedule_turn_went(Some("same"), &session, None);
        assert_eq!(
            daemon.schedules.lock().kept.runs[1].outcome,
            RunOutcome::Restarted { session }
        );
    }
}
