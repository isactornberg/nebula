//! PUSH: a notification on every phone paired to `nebula serve`'s page
//! ([`nebula_forge::push`]) when a session turns red, the BOARD's `needs
//! you`: a dialog open in it, or a finished turn whose REPORT LINE is a
//! `QUESTION` or `BLOCKED`. Read from the session's status and screen alone,
//! as the board reads them, never from Linear or a pull request, so it
//! works with no page open.
//!
//! One goes for each time a session turns red, a RED EPISODE: a session
//! that leaves red and comes back, or moves from one red state to another,
//! has a new one. It goes only once this Mac has had no keyboard or mouse
//! input for [`AWAY`] (`HIDIdleTime`), and only while the session is still
//! red, so a row that turns red while the user is at the Mac waits until
//! they step away; where the idle time does not read, it goes at once. The
//! episodes sent are kept in `push-sent.json` in the data dir, so a daemon
//! that restarts does not send them again.
//!
//! `nebula serve` keeps the subscriptions, a file each under `push/` in the
//! data dir, and the VAPID key in `vapid.json`; this reads them on every
//! beat, and drops a subscription the push service says is gone.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use nebula_core::why::{report_word, screen_text, why_line};
use nebula_core::{paths, AgentId, AgentRole, AgentStatus, SessionRef};
use nebula_forge::push::{self, Notice, Sent, Vapid};

use crate::registry::Daemon;

/// How often the sessions are looked at.
const BEAT: Duration = Duration::from_secs(1);
/// How long the Mac must have had no input for a push to go.
pub(crate) const AWAY: Duration = Duration::from_secs(60);
/// How long a state stands before its screen is read, as the board waits a
/// beat: a permission dialog draws a moment after the hook that reported it.
const SETTLE_MS: i64 = 1_000;
/// What a notification says of a session waiting on a dialog that its
/// screen does not say.
const WAITS: &str = "It is waiting on you.";
/// How long `ioreg` may take to say the idle time.
const IDLE_TIMEOUT: Duration = Duration::from_secs(2);

/// A live session as the trigger looks at it: its state, and the title a
/// notification of it has.
#[derive(Debug, Clone)]
pub(crate) struct Seen {
    pub(crate) id: AgentId,
    pub(crate) status: AgentStatus,
    /// When it moved into `status`, epoch ms: with `status`, its state.
    pub(crate) since: i64,
    pub(crate) title: String,
}

/// A red episode to send: who and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Red {
    pub(crate) id: AgentId,
    pub(crate) title: String,
    pub(crate) why: String,
}

/// What was read of a session in the state it is in.
struct Looked {
    status: AgentStatus,
    since: i64,
    /// Why it is red, when it is.
    why: Option<String>,
    title: String,
    sent: bool,
}

/// The red episodes, and which were sent.
#[derive(Default)]
pub(crate) struct Trigger {
    looked: HashMap<AgentId, Looked>,
    /// The state each session was in when it was last sent, as a daemon
    /// before this one kept it: one still in that state is not sent again.
    sent_before: HashMap<AgentId, i64>,
    /// No idle time is read before this, epoch ms: the idle time last read
    /// cannot reach [`AWAY`] sooner.
    idle_after: i64,
}

impl Trigger {
    pub(crate) fn new(sent_before: HashMap<AgentId, i64>) -> Trigger {
        Trigger {
            sent_before,
            ..Trigger::default()
        }
    }

    /// The episodes due a push at `now`, epoch ms, among the sessions
    /// `seen`: `look` reads a session's screen once per state, a beat into
    /// it, and says why it is red if it is; `idle` says how long the Mac
    /// has had no input, asked only while an episode waits.
    pub(crate) fn step(
        &mut self,
        now: i64,
        seen: &[Seen],
        mut look: impl FnMut(&Seen) -> Option<String>,
        idle: impl FnOnce() -> Option<Duration>,
    ) -> Vec<Red> {
        self.looked.retain(|id, l| {
            seen.iter()
                .any(|s| &s.id == id && s.status == l.status && s.since == l.since)
        });
        for s in seen {
            let may_be_red = matches!(s.status, AgentStatus::NeedsFeedback | AgentStatus::Finished);
            if !may_be_red || self.looked.contains_key(&s.id) || now - s.since < SETTLE_MS {
                continue;
            }
            let why = look(s);
            let sent = self.sent_before.get(&s.id) == Some(&s.since);
            self.looked.insert(
                s.id.clone(),
                Looked {
                    status: s.status,
                    since: s.since,
                    why,
                    title: s.title.clone(),
                    sent,
                },
            );
        }
        let waiting = self.looked.values().any(|l| l.why.is_some() && !l.sent);
        if !waiting || now < self.idle_after {
            return Vec::new();
        }
        if let Some(idle) = idle().filter(|idle| *idle < AWAY) {
            self.idle_after = now + (AWAY - idle).as_millis() as i64;
            return Vec::new();
        }
        let mut due: Vec<(&AgentId, &mut Looked)> = self
            .looked
            .iter_mut()
            .filter(|(_, l)| l.why.is_some() && !l.sent)
            .collect();
        due.sort_by_key(|(_, l)| l.since);
        due.into_iter()
            .map(|(id, l)| {
                l.sent = true;
                Red {
                    id: id.clone(),
                    title: l.title.clone(),
                    why: l.why.clone().unwrap_or_default(),
                }
            })
            .collect()
    }

    /// The state each session red now was in when it was sent: what a
    /// daemon after this one must not send again.
    pub(crate) fn sent(&self) -> HashMap<AgentId, i64> {
        self.looked
            .iter()
            .filter(|(_, l)| l.sent && l.why.is_some())
            .map(|(id, l)| (id.clone(), l.since))
            .collect()
    }
}

/// Why a session in `status` whose screen is `rows` is red, if it is: what
/// its dialog asks, or its report line.
pub(crate) fn why_red(status: AgentStatus, rows: &[String]) -> Option<String> {
    match status {
        AgentStatus::NeedsFeedback => {
            Some(why_line(status, rows).unwrap_or_else(|| WAITS.to_string()))
        }
        AgentStatus::Finished => why_line(status, rows).filter(|l| report_word(l).is_some()),
        _ => None,
    }
}

/// How long the Mac has had no keyboard or mouse input, by `ioreg`'s
/// `HIDIdleTime`; `None` where it does not read.
fn mac_idle() -> Option<Duration> {
    let mut child = std::process::Command::new("ioreg")
        .args(["-r", "-c", "IOHIDSystem", "-d", "1"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let started = std::time::Instant::now();
    while child.try_wait().ok()?.is_none() {
        if started.elapsed() > IDLE_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    idle_in(&out)
}

/// The `HIDIdleTime`, in nanoseconds, that `ioreg`'s `text` says.
fn idle_in(text: &str) -> Option<Duration> {
    text.lines()
        .find_map(|l| l.trim().strip_prefix("\"HIDIdleTime\" = "))
        .and_then(|n| n.trim().parse().ok())
        .map(Duration::from_nanos)
}

fn read_sent(file: &Path) -> HashMap<AgentId, i64> {
    std::fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

impl Daemon {
    /// Look at the sessions on every beat, and push each red episode to
    /// every paired phone, until the daemon stops.
    pub async fn push_red(self: &Arc<Self>) {
        let data = paths::data_dir();
        let (dir, key, file) = (
            push::push_dir(&data),
            push::vapid_file(&data),
            data.join("push-sent.json"),
        );
        let trigger = Arc::new(parking_lot::Mutex::new(Trigger::new(read_sent(&file))));
        let mut sent = trigger.lock().sent();
        let mut beat = tokio::time::interval(BEAT);
        beat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = self.shutdown.cancelled() => break,
                _ = beat.tick() => {}
            }
            let phones = push::kept(&dir);
            if phones.is_empty() {
                continue;
            }
            let Some(vapid) = Vapid::read(&key) else {
                continue;
            };
            let daemon = self.clone();
            let stepping = trigger.clone();
            let Ok(due) = tokio::task::spawn_blocking(move || {
                let seen = daemon.push_seen()?;
                let now = nebula_core::clock::now_ms();
                let mut trigger = stepping.lock();
                Some(trigger.step(now, &seen, |s| daemon.red_why(s), mac_idle))
            })
            .await
            else {
                continue;
            };
            for red in due.unwrap_or_default() {
                let notice = Notice {
                    title: red.title.clone(),
                    body: red.why.clone(),
                    tag: format!("session-{}", red.id),
                    url: format!("/s/{}", red.id),
                };
                for phone in &phones {
                    match push::send(&vapid, &phone.subscription, &notice).await {
                        Sent::Delivered => {
                            tracing::info!(session = %red.id, device = %phone.device, "pushed a red session")
                        }
                        Sent::Gone => {
                            tracing::info!(device = %phone.device, "push subscription gone: dropped");
                            let _ = std::fs::remove_file(&phone.file);
                        }
                        Sent::Failed(why) => {
                            tracing::warn!(session = %red.id, device = %phone.device, why, "push failed")
                        }
                    }
                }
            }
            let now_sent = trigger.lock().sent();
            if now_sent != sent {
                match serde_json::to_vec(&now_sent) {
                    Ok(json) => {
                        if let Err(e) = push::write_private(&file, &json) {
                            tracing::warn!(error = %e, "push-sent.json not written");
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "push-sent.json not written"),
                }
                sent = now_sent;
            }
        }
    }

    /// Every live session the board could list, as the trigger sees it:
    /// none archived, none the orchestrator's, each titled by its project
    /// and name. `None` when the store does not read.
    fn push_seen(&self) -> Option<Vec<Seen>> {
        let (projects, worktrees, agents, _) = self.store.load_tree().ok()?;
        let home = paths::orchestrator_home();
        Some(
            agents
                .into_iter()
                .filter(|a| !a.archived && a.role != AgentRole::Orchestrator)
                .filter_map(|a| {
                    let worktree = worktrees
                        .iter()
                        .find(|w| w.id == a.worktree_id && !w.removed)?;
                    let project = projects
                        .iter()
                        .find(|p| p.id == worktree.project_id && p.repo_path != home)?;
                    Some(Seen {
                        title: format!("{} › {}", project.name, a.name),
                        id: a.id,
                        status: a.status,
                        since: a.status_changed_at,
                    })
                })
                .collect(),
        )
    }

    /// Why `s` is red now, read off its screen.
    fn red_why(&self, s: &Seen) -> Option<String> {
        let rows = self
            .session(&SessionRef::Agent(s.id.clone()))
            .map(|session| {
                let tail = session.tail(crate::orchestrator::NOTE_SCREEN_BYTES, None);
                screen_text(&tail.data, tail.cols, tail.rows)
            })
            .unwrap_or_default();
        why_red(s.status, &rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(id: &str, status: AgentStatus, since: i64) -> Seen {
        Seen {
            id: AgentId(id.into()),
            status,
            since,
            title: format!("shopfront › {id}"),
        }
    }

    /// A trigger stepped through time: the clock and the Mac's idle time
    /// are the test's, and every screen reads as red but a `DONE` one.
    struct Run {
        trigger: Trigger,
        looks: usize,
        idle_reads: usize,
    }

    impl Run {
        fn new() -> Run {
            Run {
                trigger: Trigger::default(),
                looks: 0,
                idle_reads: 0,
            }
        }

        fn step(&mut self, now: i64, seen: &[Seen], idle: Option<u64>) -> Vec<String> {
            let looks = &mut self.looks;
            let idle_reads = &mut self.idle_reads;
            self.trigger
                .step(
                    now,
                    seen,
                    |s| {
                        *looks += 1;
                        (s.status == AgentStatus::NeedsFeedback || s.id.0.starts_with("asks"))
                            .then(|| format!("why {}", s.id))
                    },
                    || {
                        *idle_reads += 1;
                        idle.map(Duration::from_secs)
                    },
                )
                .into_iter()
                .map(|r| r.id.0)
                .collect()
        }
    }

    const AWAY_S: u64 = AWAY.as_secs();

    #[test]
    fn a_red_session_is_pushed_once_while_the_mac_is_idle() {
        let mut run = Run::new();
        let red = [seen("a1", AgentStatus::NeedsFeedback, 10_000)];
        assert!(
            run.step(10_500, &red, Some(AWAY_S)).is_empty(),
            "not read before a beat"
        );
        assert_eq!(run.looks, 0);
        assert_eq!(run.step(11_000, &red, Some(AWAY_S)), ["a1"]);
        for now in [12_000, 60_000, 600_000] {
            assert!(run.step(now, &red, Some(AWAY_S * 10)).is_empty(), "once");
        }
        assert_eq!(run.looks, 1, "the screen is read once per state");
        assert_eq!(run.idle_reads, 1, "nothing waits after it is sent");
    }

    #[test]
    fn a_session_that_leaves_red_and_comes_back_is_a_new_episode() {
        let mut run = Run::new();
        let at = |status, since| [seen("a1", status, since)];
        assert_eq!(
            run.step(2_000, &at(AgentStatus::NeedsFeedback, 1_000), None),
            ["a1"]
        );
        assert!(run
            .step(3_000, &at(AgentStatus::Running, 2_500), None)
            .is_empty());
        assert_eq!(
            run.step(5_000, &at(AgentStatus::NeedsFeedback, 4_000), None),
            ["a1"],
            "red again"
        );
        // A dialog answered and the next one asked between two beats.
        assert_eq!(
            run.step(7_000, &at(AgentStatus::NeedsFeedback, 6_000), None),
            ["a1"]
        );
    }

    #[test]
    fn a_finish_is_red_only_when_its_report_line_asks() {
        let mut run = Run::new();
        let seen = [
            seen("asks-q", AgentStatus::Finished, 1_000),
            seen("done-d", AgentStatus::Finished, 1_000),
            seen("run-r", AgentStatus::Running, 1_000),
            seen("fresh-f", AgentStatus::Fresh, 1_000),
        ];
        assert_eq!(run.step(5_000, &seen, None), ["asks-q"]);
        assert_eq!(run.looks, 2, "running and fresh sessions are not read");
    }

    #[test]
    fn a_session_that_turns_red_at_the_mac_waits_until_it_is_left_alone() {
        let mut run = Run::new();
        let red = [seen("a1", AgentStatus::NeedsFeedback, 0)];
        // Typing: 5 s since the last key.
        assert!(run.step(1_000, &red, Some(5)).is_empty());
        assert_eq!(run.idle_reads, 1);
        // Idle time cannot reach a minute before 55 s more: not read again.
        for now in [2_000, 30_000, 55_999] {
            assert!(run.step(now, &red, Some(AWAY_S)).is_empty(), "{now}");
        }
        assert_eq!(run.idle_reads, 1);
        // At the Mac again meanwhile, 10 s ago.
        assert!(run.step(56_000, &red, Some(10)).is_empty());
        assert_eq!(run.idle_reads, 2);
        assert!(run.step(105_999, &red, Some(AWAY_S)).is_empty());
        // Gone for a minute, and still red: sent.
        assert_eq!(run.step(106_000, &red, Some(AWAY_S)), ["a1"]);
        assert_eq!(run.idle_reads, 3);
    }

    #[test]
    fn a_session_answered_at_the_mac_before_its_user_steps_away_is_not_sent() {
        let mut run = Run::new();
        let red = [seen("a1", AgentStatus::NeedsFeedback, 0)];
        assert!(run.step(1_000, &red, Some(0)).is_empty());
        let answered = [seen("a1", AgentStatus::Running, 20_000)];
        assert!(run.step(61_000, &answered, Some(AWAY_S)).is_empty());
        assert_eq!(run.idle_reads, 1, "nothing waits: no idle time read");
        let gone: [Seen; 0] = [];
        assert!(run.step(70_000, &gone, Some(AWAY_S)).is_empty());
    }

    #[test]
    fn with_no_idle_time_to_read_a_red_session_is_sent_at_once() {
        let mut run = Run::new();
        let red = [
            seen("a1", AgentStatus::NeedsFeedback, 0),
            seen("a2", AgentStatus::NeedsFeedback, -500),
        ];
        assert_eq!(run.step(1_000, &red, None), ["a2", "a1"], "oldest first");
    }

    #[test]
    fn an_episode_a_daemon_before_this_one_sent_is_not_sent_again() {
        let mut first = Run::new();
        let red = [
            seen("a1", AgentStatus::NeedsFeedback, 0),
            seen("a2", AgentStatus::Finished, 0),
        ];
        assert_eq!(first.step(1_000, &red, None), ["a1"]);
        let sent = first.trigger.sent();
        assert_eq!(sent, HashMap::from([(AgentId("a1".into()), 0)]));

        let mut next = Run::new();
        next.trigger = Trigger::new(sent);
        assert!(next.step(2_000, &red, None).is_empty());
        let again = [seen("a1", AgentStatus::NeedsFeedback, 1_500)];
        assert_eq!(next.step(3_000, &again, None), ["a1"], "a new episode");
    }

    fn rows(text: &str) -> Vec<String> {
        text.lines().map(str::to_string).collect()
    }

    #[test]
    fn red_is_a_dialog_or_a_report_line_that_asks() {
        let dialog = rows(
            "⏺ I'll rotate the key.\n\n Bash command\n\n   rotate-key --env staging\n\n Do you want to proceed?\n ❯ 1. Yes\n   2. No",
        );
        assert_eq!(
            why_red(AgentStatus::NeedsFeedback, &dialog).as_deref(),
            Some("Bash: rotate-key --env staging")
        );
        assert_eq!(
            why_red(AgentStatus::NeedsFeedback, &[]).as_deref(),
            Some(WAITS)
        );
        let asks = rows("> Fix it\n\n⏺ QUESTION: which base branch should the fix go on?\n\n> ");
        assert_eq!(
            why_red(AgentStatus::Finished, &asks).as_deref(),
            Some("QUESTION: which base branch should the fix go on?")
        );
        let done = rows("> Fix it\n\n⏺ DONE fixed the login loop\n\n> ");
        assert_eq!(why_red(AgentStatus::Finished, &done), None);
        assert_eq!(why_red(AgentStatus::Running, &asks), None);
    }

    #[test]
    fn the_idle_time_is_read_from_ioreg_in_nanoseconds() {
        let out = "+-o IOHIDSystem  <class IOHIDSystem>\n    {\n      \"HIDScrollCountMaxTimeDeltaBetween\" = 600\n      \"HIDIdleTime\" = 122744388625\n    }\n";
        assert_eq!(idle_in(out), Some(Duration::from_nanos(122_744_388_625)));
        assert_eq!(idle_in("nothing"), None);
    }
}
