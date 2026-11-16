//! SCHEDULES, as the daemon keeps them and tells every client of them: a
//! prompt in a project, or the orchestrator's RANK TURN, run on a form of
//! time ([`ScheduleWhen`]), and the RUN LOG of what each run did. The
//! daemon owns the timing, the runs and the file they are kept in
//! (`nebula-daemon`'s `schedule`); these are the shapes on the wire and in
//! that file.

use crate::{AgentId, AgentKind, ProjectId};
use serde::{Deserialize, Serialize};

/// The days a time-of-day schedule runs on, one bit a day, Monday the
/// lowest: [`DAYS_ALL`] is daily, [`DAYS_WEEKDAYS`] Monday to Friday.
pub type Days = u8;
pub const DAYS_ALL: Days = 0x7f;
pub const DAYS_WEEKDAYS: Days = 0x1f;
/// The days' short names, Monday first, as `--days` takes and prints them.
pub const DAY_NAMES: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// The modes Claude Code's `--permission-mode` takes (2.1.325) that a
/// schedule may name; none is `default`, as nebula launches Claude, which
/// is the mode your own Claude settings give it. `manual` asks for every
/// permission, whatever those settings say.
pub const PERMISSION_MODES: &[&str] = &[
    "manual",
    "acceptEdits",
    "auto",
    "plan",
    "dontAsk",
    "bypassPermissions",
];

/// What `nebula schedule add` and the page's add hand the daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleSpec {
    /// What the schedule and its sessions are called; empty takes one from
    /// the prompt.
    pub name: String,
    pub task: ScheduleTask,
    pub when: ScheduleWhen,
}

/// What a run does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleTask {
    /// A prompt for an agent session in `project`, on `kind` (with its
    /// registry id when custom), `model` and `effort` (none: the CLI's own),
    /// and `permission_mode`, Claude's `--permission-mode` (none: as nebula
    /// launches the harness), going where `target` says.
    Prompt {
        project: ProjectId,
        prompt: String,
        kind: AgentKind,
        custom_harness: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        permission_mode: Option<String>,
        target: ScheduleTarget,
    },
    /// The ORCHESTRATOR's RANK TURN, which ranks the web app's focus picks,
    /// with `nebula serve` started first when none runs.
    Rank,
}

/// Where each run of a prompt goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleTarget {
    /// A new session in a new worktree of its own.
    NewWorktree,
    /// A new session in the project's root checkout.
    Root,
    /// One session the first run starts in the root checkout, typed each
    /// later run's prompt once it is at rest with an empty input box.
    Session,
    /// That session, its CLI started again on a new conversation for each
    /// later run, the prompt its first.
    FreshSession,
}

impl ScheduleTarget {
    pub const ALL: [ScheduleTarget; 4] = [
        ScheduleTarget::NewWorktree,
        ScheduleTarget::Root,
        ScheduleTarget::Session,
        ScheduleTarget::FreshSession,
    ];

    /// The name `--in` takes.
    pub fn as_str(self) -> &'static str {
        match self {
            ScheduleTarget::NewWorktree => "new-worktree",
            ScheduleTarget::Root => "root",
            ScheduleTarget::Session => "session",
            ScheduleTarget::FreshSession => "fresh-session",
        }
    }

    pub fn parse(s: &str) -> Option<ScheduleTarget> {
        Self::ALL.into_iter().find(|t| t.as_str() == s)
    }

    /// Whether its runs after the first go to the one session.
    pub fn same_session(self) -> bool {
        matches!(self, ScheduleTarget::Session | ScheduleTarget::FreshSession)
    }
}

/// When a schedule runs. No cron syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleWhen {
    /// Every `minutes` minutes, the first that long after it was added.
    Every { minutes: u32 },
    /// At `hour`:`minute` on the Mac's clock, on `days`.
    At { hour: u8, minute: u8, days: Days },
    /// Once, at `at`, epoch ms.
    Once { at: i64 },
}

/// A schedule as the daemon keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    pub id: String,
    pub spec: ScheduleSpec,
    /// When it was added, epoch ms.
    pub created_at: i64,
    /// When it runs next, epoch ms; none once a once schedule has run.
    pub next_at: Option<i64>,
    /// For a same-session target: the session its runs go to, once its
    /// first run started one.
    pub session: Option<AgentId>,
}

/// One run, or one run skipped, in the RUN LOG.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleRun {
    pub schedule: String,
    /// When it was due, epoch ms; none for a run asked for now.
    pub due_at: Option<i64>,
    /// When it ran, or was skipped, epoch ms.
    pub at: i64,
    pub outcome: RunOutcome,
    /// Why it ran when it did, when that was not on time: a late interval
    /// run that stands for the ones it missed, a daily schedule's catch-up.
    pub note: Option<String>,
}

/// What a run did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    /// A new session, on the prompt.
    Started {
        session: AgentId,
    },
    /// The turn waits for `session` to be at rest with an empty input box.
    Held {
        session: AgentId,
    },
    /// The turn was typed into `session`.
    Typed {
        session: AgentId,
    },
    /// `session`'s CLI was started again on a new conversation, the prompt
    /// its first.
    Restarted {
        session: AgentId,
    },
    Skipped {
        why: String,
    },
    Failed {
        why: String,
    },
}

impl RunOutcome {
    /// The session the run went to, if it went to one.
    pub fn session(&self) -> Option<&AgentId> {
        match self {
            RunOutcome::Started { session }
            | RunOutcome::Held { session }
            | RunOutcome::Typed { session }
            | RunOutcome::Restarted { session } => Some(session),
            RunOutcome::Skipped { .. } | RunOutcome::Failed { .. } => None,
        }
    }
}

/// Every schedule, and the RUN LOG, newest run last, as every client is
/// told of them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleBook {
    pub schedules: Vec<Schedule>,
    pub runs: Vec<ScheduleRun>,
}

/// `days` as `--days` names it: `daily`, `weekdays`, or the days,
/// `mon,wed,fri`.
pub fn days_label(days: Days) -> String {
    match days & DAYS_ALL {
        DAYS_ALL => "daily".into(),
        DAYS_WEEKDAYS => "weekdays".into(),
        d => DAY_NAMES
            .iter()
            .enumerate()
            .filter(|(i, _)| d & (1 << i) != 0)
            .map(|(_, name)| *name)
            .collect::<Vec<_>>()
            .join(","),
    }
}

/// The days `--days` names, as [`days_label`] writes them, any case, with
/// `weekends` too; `None` for a name it does not know or no day at all.
pub fn parse_days(text: &str) -> Option<Days> {
    let mut days = 0;
    for word in text.split([',', ' ']).filter(|w| !w.is_empty()) {
        days |= match word.to_ascii_lowercase().as_str() {
            "daily" | "every" | "all" => DAYS_ALL,
            "weekdays" => DAYS_WEEKDAYS,
            "weekends" => DAYS_ALL & !DAYS_WEEKDAYS,
            day => 1 << DAY_NAMES.iter().position(|n| day.starts_with(n))?,
        };
    }
    (days != 0).then_some(days)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_read_as_they_are_written() {
        assert_eq!(days_label(DAYS_ALL), "daily");
        assert_eq!(days_label(DAYS_WEEKDAYS), "weekdays");
        assert_eq!(days_label(0b101_0101), "mon,wed,fri,sun");
        for days in [DAYS_ALL, DAYS_WEEKDAYS, 0b101_0101, 1, 0b100_0000] {
            assert_eq!(parse_days(&days_label(days)), Some(days));
        }
        assert_eq!(parse_days("Monday, Friday"), Some(0b1_0001));
        assert_eq!(parse_days("weekends"), Some(0b110_0000));
        assert_eq!(parse_days("someday"), None);
        assert_eq!(parse_days(""), None);
    }

    #[test]
    fn a_target_reads_as_its_name() {
        for target in ScheduleTarget::ALL {
            assert_eq!(ScheduleTarget::parse(target.as_str()), Some(target));
        }
        assert!(ScheduleTarget::FreshSession.same_session());
        assert!(!ScheduleTarget::Root.same_session());
    }
}
