//! FOCUS: the page's picks, at most three. By rule they are the board's rows
//! that need you, then the ones done ([`crate::api::state`]); once the
//! ORCHESTRATOR has ranked, they are its own, each with why. It ranks the
//! CANDIDATES `nebula serve` holds, the rows that need you or are done and
//! the issues in the lanes, which it reads with `nebula orchestrator
//! attention --json` ([`attention`]) and picks from with `nebula
//! orchestrator picks set --json -` ([`validate`]), both through this
//! server, which stamps the picks and keeps them, with what the user said
//! of them, in `focus.json` in the data dir ([`Kept`]).
//!
//! The page asks for a rank on Re-rank, and on opening when the picks are
//! DUE ([`due`]); the daemon types the RANK TURN into the orchestrator once
//! it is at rest with an empty input box (`ClientRequest::RankFocus`).
//! Picks over three hours old with the candidates changed since are stale,
//! and the rule stands in for them, as it does for picks that are no
//! candidates now or that the user put aside ([`shown`]).

use crate::api::{
    self, BoardSection, Candidate, CandidateIssue, CandidateSession, FocusFallback, FocusRank,
    IssueLane, IssueLaneName, Lane, RankState, Said, SectionName, Session, SessionStatus, Verdict,
};
use crate::hub::{Hub, Live};
use crate::refresh::Refresher;
use crate::tailnet::Visitor;
use crate::{sentence, App, Refusal};
use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

/// The most picks there are.
pub const PICKS: usize = 3;
/// How old picks are before a change in the candidates makes them stale.
pub const STALE_MS: i64 = 3 * 3600 * 1000;
/// How many ranks the page may ask for on its own in a day.
pub const AUTO_RUNS: usize = 4;
/// How old Linear and the pull requests may be when attention is read
/// before it reads them again.
pub const FRESH_MS: i64 = 10 * 60 * 1000;
/// How long a rank asked for may go unheard of from the daemon before it
/// counts as gone.
pub const ASK_GRACE_MS: i64 = 30 * 1000;
/// How long attention, and the page's own ask on opening, wait for a read
/// of Linear and the pull requests to land.
const READ_WAIT: std::time::Duration = std::time::Duration::from_secs(60);
/// The header `nebula orchestrator attention` and `picks set` name the
/// session they run as in.
pub const AGENT_HEADER: &str = "x-nebula-agent";
/// How many candidates attention prints, the most urgent.
const PRINTED: usize = 25;
/// How long attention carries what the user said of a pick, and how much.
const SAID_MS: i64 = 7 * 24 * 3600 * 1000;
const SAID_PRINTED: usize = 20;
/// How long the file keeps it, and how much.
const KEPT_MS: i64 = 14 * 24 * 3600 * 1000;
const KEPT_SAID: usize = 100;
/// The longest reason a pick or a correction may have.
pub const REASON_MAX: usize = 300;

/// What the focus keeps: in `focus.json`, the orchestrator's last picks,
/// what the user said of picks, and when the page asked for a rank on its
/// own; in memory, the candidates the last attention was read with and the
/// rank asked for last.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Kept {
    pub picks: Option<Picks>,
    pub said: Vec<Feedback>,
    /// The ranks asked for on the page's own, epoch ms.
    pub runs: Vec<i64>,
    #[serde(skip)]
    pub seen: Vec<Seen>,
    #[serde(skip)]
    pub asked: Option<Asked>,
}

/// The orchestrator's picks, when it set them, and the candidates it
/// ranked them from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Picks {
    pub at: i64,
    pub picks: Vec<Pick>,
    pub ranked: Vec<Seen>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pick {
    pub key: String,
    pub reason: String,
}

/// A candidate as it stood: its key and lane.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Seen {
    pub key: String,
    pub lane: Lane,
}

/// What the user said of a pick, Not today or Wrong.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feedback {
    pub key: String,
    pub verdict: Verdict,
    pub reason: Option<String>,
    pub at: i64,
    /// The pick's reason, and what it was.
    pub picked: String,
    pub what: String,
}

/// A rank asked of the daemon, and whether the page asked on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asked {
    pub at: i64,
    pub auto: bool,
}

impl Kept {
    /// What `file` keeps: nothing when there is no file or it does not read.
    pub fn load(file: &Path) -> Kept {
        std::fs::read(file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Write it to `file`, renamed into place so a reader never sees half
    /// of it, dropping what is too old to say any more.
    pub fn save(&mut self, file: &Path, now: i64) {
        self.said.retain(|f| now - f.at < KEPT_MS);
        let over = self.said.len().saturating_sub(KEPT_SAID);
        self.said.drain(..over);
        let today = day_start(now);
        self.runs.retain(|&at| at >= today);
        let tmp = file.with_extension(format!("json.{}", std::process::id()));
        let written = serde_json::to_vec_pretty(self)
            .map_err(std::io::Error::other)
            .and_then(|json| std::fs::write(&tmp, json))
            .and_then(|()| std::fs::rename(&tmp, file));
        if let Err(err) = written {
            eprintln!("nebula serve: {} not written: {err}", file.display());
        }
    }
}

/// A candidate for a pick, as the STATE has it.
pub struct Found<'a> {
    pub key: String,
    pub lane: Lane,
    /// The row's session first, then the issue's others.
    pub sessions: Vec<&'a Session>,
    pub issue: Option<&'a api::Issue>,
}

impl Found<'_> {
    /// What it is: its issue's title, else its session's name.
    pub fn what(&self) -> String {
        match (self.issue, self.sessions.first()) {
            (Some(i), _) => format!("{} {}", i.key, i.title),
            (None, Some(s)) => s.name.clone(),
            (None, None) => self.key.clone(),
        }
    }

    fn seen(&self) -> Seen {
        Seen {
            key: self.key.clone(),
            lane: self.lane,
        }
    }
}

/// The CANDIDATES: the board's rows that need you, then the ones done, in
/// its order, each keyed by the issue joined to it, with the issue's other
/// sessions, or by its session; then the issues in the lanes, in flight and
/// then todo. A key in `off` is left out, as the user said not today to it.
pub fn candidates<'a>(
    sessions: &'a [Session],
    sections: &[BoardSection],
    lanes: &'a [IssueLane],
    off: &HashSet<String>,
) -> Vec<Found<'a>> {
    let mut found: Vec<Found> = Vec::new();
    for section in sections {
        let lane = match section.section {
            SectionName::NeedsYou => Lane::NeedsYou,
            SectionName::Done => Lane::Done,
            SectionName::Running | SectionName::Captains => continue,
        };
        for row in &section.rows {
            let Some(s) = sessions.iter().find(|s| s.id == row.session_id) else {
                continue;
            };
            let key = s
                .issue
                .as_ref()
                .map_or_else(|| s.id.clone(), |i| i.key.clone());
            if off.contains(&key) || found.iter().any(|f| f.key == key) {
                continue;
            }
            let mut on: Vec<&Session> = match &s.issue {
                Some(issue) => sessions
                    .iter()
                    .filter(|x| x.issue.as_ref().is_some_and(|i| i.key == issue.key))
                    .collect(),
                None => vec![s],
            };
            on.sort_by_key(|x| x.id != s.id);
            found.push(Found {
                key,
                lane,
                sessions: on,
                issue: s.issue.as_ref(),
            });
        }
    }
    for lane in lanes {
        let name = match lane.lane {
            IssueLaneName::InFlight => Lane::InFlight,
            IssueLaneName::Todo => Lane::Todo,
        };
        for issue in lane.issues.iter().filter(|i| !off.contains(&i.key)) {
            found.push(Found {
                key: issue.key.clone(),
                lane: name,
                sessions: Vec::new(),
                issue: Some(issue),
            });
        }
    }
    found
}

/// The keys the user said not today to, today.
pub fn off(kept: &Kept, now: i64) -> HashSet<String> {
    let today = day_start(now);
    kept.said
        .iter()
        .filter(|f| f.verdict == Verdict::NotToday && f.at >= today)
        .map(|f| f.key.clone())
        .collect()
}

/// What the page shows of the orchestrator's picks.
#[derive(Debug, PartialEq)]
pub enum Shown<'k> {
    /// The rule's, for want of the orchestrator's, and why when it has
    /// picked before.
    Rule(Option<FocusFallback>),
    /// Its picks that are candidates now and that the user has not put
    /// aside since.
    Orchestrator(&'k Picks, Vec<&'k Pick>),
}

/// The orchestrator's picks as `candidates` stand at `now`: the rule's in
/// their place when it has not ranked, before Linear and the pull requests
/// are `read`, which the candidates are keyed by, when they are STALE, over
/// three hours old with the candidates changed since, when none is a
/// candidate now, and when the user put each one aside.
pub fn shown<'k>(kept: &'k Kept, candidates: &[Found], read: bool, now: i64) -> Shown<'k> {
    let Some(picks) = &kept.picks else {
        return Shown::Rule(None);
    };
    if !read {
        return Shown::Rule(Some(FocusFallback::Unread));
    }
    if now - picks.at > STALE_MS && changed(&picks.ranked, candidates) {
        return Shown::Rule(Some(FocusFallback::Stale));
    }
    let on_board: Vec<&Pick> = picks
        .picks
        .iter()
        .filter(|p| candidates.iter().any(|c| c.key == p.key))
        .collect();
    if on_board.is_empty() && !picks.picks.is_empty() {
        return Shown::Rule(Some(FocusFallback::Gone));
    }
    let set_aside = |p: &Pick| kept.said.iter().any(|f| f.key == p.key && f.at >= picks.at);
    let live: Vec<&Pick> = on_board.iter().copied().filter(|p| !set_aside(p)).collect();
    if live.is_empty() && !on_board.is_empty() {
        return Shown::Rule(Some(FocusFallback::SetAside));
    }
    Shown::Orchestrator(picks, live)
}

/// Do `candidates` differ from the ones `ranked` holds, a key or a lane?
fn changed(ranked: &[Seen], candidates: &[Found]) -> bool {
    let then: HashSet<&Seen> = ranked.iter().collect();
    let now: Vec<Seen> = candidates.iter().map(Found::seen).collect();
    then != now.iter().collect()
}

/// Are the picks DUE a rank the page asks for on its own: never ranked,
/// or stale, and fewer than [`AUTO_RUNS`] asked for so today?
pub fn due(kept: &Kept, candidates: &[Found], now: i64) -> bool {
    let today = day_start(now);
    if kept.runs.iter().filter(|&&at| at >= today).count() >= AUTO_RUNS {
        return false;
    }
    kept.picks
        .as_ref()
        .is_none_or(|picks| now - picks.at > STALE_MS && changed(&picks.ranked, candidates))
}

/// Where the rank asked for last stands, by the daemon's `turn` and the
/// orchestrator's status and when it changed: none once picks came after
/// it, and none once the daemon has neither held nor typed it for
/// [`ASK_GRACE_MS`], as after a restart that lost it.
pub fn rank(
    kept: &Kept,
    turn: nebula_core::RankTurn,
    orchestrator: Option<(SessionStatus, i64)>,
    now: i64,
) -> Option<FocusRank> {
    let asked = kept.asked?;
    if kept.picks.as_ref().is_some_and(|p| p.at >= asked.at) {
        return None;
    }
    let state = match turn.typed_at.filter(|&at| at >= asked.at) {
        _ if turn.held_at.is_some() => RankState::Held,
        Some(typed) => match orchestrator {
            Some((status, at)) if at > typed && at_rest(status) => RankState::Ended,
            _ => RankState::Ranking,
        },
        None if now - asked.at < ASK_GRACE_MS => RankState::Held,
        None => return None,
    };
    Some(FocusRank {
        at: asked.at,
        auto: asked.auto,
        state,
    })
}

fn at_rest(status: SessionStatus) -> bool {
    !matches!(
        status,
        SessionStatus::Running | SessionStatus::NeedsFeedback
    )
}

/// What `nebula orchestrator attention --json` prints of `candidates` at
/// `now`: the first [`PRINTED`], with what the user said of picks in the
/// last week, and how old the issues and pull requests are, with why a part
/// could not be read.
pub fn attention(
    candidates: &[Found],
    kept: &Kept,
    refresh: &api::Refresh,
    now: i64,
) -> api::Attention {
    let printed = candidates
        .iter()
        .take(PRINTED)
        .map(|c| Candidate {
            key: c.key.clone(),
            lane: c.lane,
            issue: c.issue.map(|i| CandidateIssue {
                title: i.title.clone(),
                state: i.state.clone(),
                priority: match i.priority {
                    1 => Some("urgent"),
                    2 => Some("high"),
                    3 => Some("medium"),
                    4 => Some("low"),
                    _ => None,
                },
                project: i.project.clone(),
                changed: age(i.updated_at, now),
                handed: i.started.as_ref().map(|s| s.project_name.clone()),
            }),
            sessions: c
                .sessions
                .iter()
                .map(|s| CandidateSession {
                    id: s.id.clone(),
                    project: s.project_name.clone(),
                    name: s.name.clone(),
                    status: s.status,
                    why: s.why.clone(),
                    since: age(s.status_changed_at, now),
                    pr: s.pr_state,
                })
                .collect(),
        })
        .collect();
    api::Attention {
        read: refresh.at.map(|at| age(at, now)),
        problems: refresh.problems.clone(),
        candidates: printed,
        more: candidates.len().saturating_sub(PRINTED),
        feedback: kept
            .said
            .iter()
            .rev()
            .filter(|f| now - f.at < SAID_MS)
            .take(SAID_PRINTED)
            .map(|f| Said {
                key: f.key.clone(),
                said: f.verdict,
                reason: f.reason.clone(),
                picked: f.picked.clone(),
                what: f.what.clone(),
                ago: age(f.at, now),
            })
            .collect(),
    }
}

/// The candidates as attention leaves them to be ranked from.
pub fn seen(candidates: &[Found]) -> Vec<Seen> {
    candidates.iter().map(Found::seen).collect()
}

/// The picks `raw` sets, a JSON array of `{"key", "reason"}`, each reason
/// on one line, or why they are refused: more than [`PICKS`], a key picked
/// twice, a reason missing or too long, and a key that is no candidate now
/// nor was one when attention was last read (`kept.seen`).
pub fn validate(
    raw: &[u8],
    kept: &Kept,
    candidates: &[Found],
    off: &HashSet<String>,
) -> Result<Vec<Pick>, String> {
    let got: Vec<Pick> = serde_json::from_slice(raw).map_err(|err| {
        format!(
            "The picks are not a JSON array of {{\"key\", \"reason\"}}: {}.",
            err.to_string().trim_end_matches('.')
        )
    })?;
    if got.len() > PICKS {
        return Err(format!("At most three picks, not {}.", got.len()));
    }
    let mut picks: Vec<Pick> = Vec::new();
    for pick in got {
        let key = pick.key.trim().to_string();
        if picks.iter().any(|p| p.key == key) {
            return Err(format!("{key} is picked twice."));
        }
        if off.contains(&key) {
            return Err(format!(
                "{key} is no candidate today: the user said not today to it."
            ));
        }
        let candidate =
            candidates.iter().any(|c| c.key == key) || kept.seen.iter().any(|s| s.key == key);
        if !candidate {
            return Err(format!(
                "{key} is no candidate: pick keys as nebula orchestrator attention --json prints them."
            ));
        }
        let reason = one_line(&pick.reason);
        if reason.is_empty() {
            return Err(format!("The pick of {key} has no reason."));
        }
        if reason.chars().count() > REASON_MAX {
            return Err(format!(
                "The reason for {key} is over {REASON_MAX} characters."
            ));
        }
        picks.push(Pick { key, reason });
    }
    Ok(picks)
}

/// `text` on one line, its runs of white space one space each.
pub fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How long ago `at` was, as the board says it: `now`, `5m`, `3h`, `2d`.
fn age(at: i64, now: i64) -> String {
    let s = (now - at).max(0) / 1000;
    match s {
        0..60 => "now".into(),
        60..3600 => format!("{}m", s / 60),
        3600..86_400 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
    }
}

/// When the day `now` is in began on this machine's clock, epoch ms: what
/// Not today lasts until the end of, and what the page's own ranks are
/// counted in.
pub fn day_start(now: i64) -> i64 {
    let secs = now.div_euclid(1000) as libc::time_t;
    // SAFETY: localtime_r writes only the `tm` it is handed.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&secs, &mut tm) }.is_null() {
        return now - now.rem_euclid(86_400_000);
    }
    let into = i64::from(tm.tm_hour) * 3600 + i64::from(tm.tm_min) * 60 + i64::from(tm.tm_sec);
    now - into * 1000 - now.rem_euclid(1000)
}

/// Refuse what is not the orchestrator's own command: anything from a
/// device on the tailnet, and a command whose session, by the
/// [`AGENT_HEADER`] it names, is not the orchestrator, as the daemon
/// refuses the orchestrator's other commands to any other session.
fn orchestrator_only(
    app: &App,
    visitor: &Visitor,
    headers: &HeaderMap,
) -> Result<Arc<Live>, Refusal> {
    let refused = |why: &str| Refusal::new(StatusCode::FORBIDDEN, why);
    if *visitor != Visitor::Local {
        return Err(refused(
            "Only the orchestrator on this Mac reads and sets the focus picks.",
        ));
    }
    let live = app.hub.live().ok_or_else(Refusal::down)?;
    let caller = headers.get(AGENT_HEADER).and_then(|v| v.to_str().ok());
    if caller.is_none() || caller != live.state.orchestrator_id.as_deref() {
        return Err(refused(
            "Only the orchestrator reads and sets the focus picks: this command is for its session.",
        ));
    }
    Ok(live)
}

/// `live` once Linear and the pull requests were read no longer than
/// [`FRESH_MS`] ago, read again first when they were not, waiting up to
/// [`READ_WAIT`] for that read; `live` as it is when `always` is false and
/// they were read at all.
async fn read_enough(hub: &Hub, refresher: &Refresher, live: Arc<Live>, always: bool) -> Arc<Live> {
    let now = nebula_core::clock::now_ms();
    let read = live.state.refresh.at;
    let enough = if always {
        read.is_some_and(|at| now - at < FRESH_MS)
    } else {
        read.is_some()
    };
    if enough && !live.state.refresh.running {
        return live;
    }
    if always || !live.state.refresh.running {
        refresher.start(crate::refresh::Ask::of(&live));
    }
    let mut states = hub.subscribe();
    let landed = states.wait_for(|l| {
        l.as_ref().is_some_and(|l| {
            !l.state.refresh.running && l.state.refresh.at.is_some_and(|at| at >= now)
        })
    });
    let _ = tokio::time::timeout(READ_WAIT, landed)
        .await
        .map(|got| got.is_ok());
    hub.live().unwrap_or(live)
}

/// `GET /api/attention`, for `nebula orchestrator attention --json`: the
/// candidates, Linear and the pull requests read again first when they are
/// older than [`FRESH_MS`]. The candidates are kept to hold the picks set
/// next to.
pub(crate) async fn attention_route(
    State(app): State<App>,
    Extension(visitor): Extension<Visitor>,
    headers: HeaderMap,
) -> Result<Json<api::Attention>, Refusal> {
    let live = orchestrator_only(&app, &visitor, &headers)?;
    let live = read_enough(&app.hub, &app.refresher, live, true).await;
    let now = nebula_core::clock::now_ms();
    let fetched = app.refresher.fetched();
    let off = off(&fetched.focus, now);
    let board = &live.state.board;
    let found = candidates(&live.state.sessions, &board.sections, &board.lanes, &off);
    let answer = attention(&found, &fetched.focus, &live.state.refresh, now);
    let seen = seen(&found);
    app.refresher.patch(|f| f.focus.seen = seen);
    Ok(Json(answer))
}

/// `POST /api/focus/picks`, for `nebula orchestrator picks set --json -`:
/// the orchestrator's picks, held to the candidates ([`validate`]), stamped
/// with the time and kept.
pub(crate) async fn picks_route(
    State(app): State<App>,
    Extension(visitor): Extension<Visitor>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<api::PicksSet>, Refusal> {
    let live = orchestrator_only(&app, &visitor, &headers)?;
    let now = nebula_core::clock::now_ms();
    let fetched = app.refresher.fetched();
    let off = off(&fetched.focus, now);
    let board = &live.state.board;
    let found = candidates(&live.state.sessions, &board.sections, &board.lanes, &off);
    let picks = validate(&body, &fetched.focus, &found, &off)
        .map_err(|why| Refusal::new(StatusCode::BAD_REQUEST, why))?;
    let said = match picks.len() {
        0 => "Set no focus picks: the page shows what needs you under them.".to_string(),
        1 => "Set 1 focus pick.".to_string(),
        n => format!("Set {n} focus picks."),
    };
    let ranked = if fetched.focus.seen.is_empty() {
        seen(&found)
    } else {
        fetched.focus.seen.clone()
    };
    app.refresher.patch(|f| {
        f.focus.picks = Some(Picks {
            at: now,
            picks,
            ranked,
        });
        f.focus.save(&app.focus, now);
    });
    Ok(Json(api::PicksSet { said }))
}

/// `POST /api/focus/rank`: ask the daemon to have the orchestrator rank,
/// on Re-rank, joining a rank under way, or, with `auto`, as the page
/// opens, only when the picks are [`due`] and the orchestrator is at rest
/// now, answered `204` when it is not asked.
pub(crate) async fn rank_route(
    State(app): State<App>,
    got: Result<Json<api::RankFocus>, JsonRejection>,
) -> Result<StatusCode, Refusal> {
    let auto = crate::body(got)?.auto;
    let _one = app.ranking.lock().await;
    let live = app.hub.live().ok_or_else(Refusal::down)?;
    let conflict = |why: String| Refusal::new(StatusCode::CONFLICT, why);
    if live.state.orchestrator_id.is_none() {
        return Err(conflict(
            "There is no orchestrator to rank the picks: F10 in the TUI starts one.".into(),
        ));
    }
    if live.protocol < nebula_core::RANK_PROTOCOL {
        return Err(conflict(
            "The running daemon cannot ask the orchestrator to rank: `nebula reload` moves it onto \
             this build, its sessions kept."
                .into(),
        ));
    }
    if live
        .state
        .focus
        .rank
        .is_some_and(|r| r.state != RankState::Ended)
    {
        return Ok(StatusCode::ACCEPTED);
    }
    if auto {
        let live = read_enough(&app.hub, &app.refresher, live, false).await;
        let now = nebula_core::clock::now_ms();
        let fetched = app.refresher.fetched();
        let board = &live.state.board;
        let off = off(&fetched.focus, now);
        let found = candidates(&live.state.sessions, &board.sections, &board.lanes, &off);
        if !due(&fetched.focus, &found, now) {
            return Ok(StatusCode::NO_CONTENT);
        }
    }
    let asked = app
        .hub
        .connect("the orchestrator was not asked to rank")
        .await?
        .rank_focus(!auto)
        .await;
    match asked {
        Ok(()) => {}
        Err(_) if auto => return Ok(StatusCode::NO_CONTENT),
        Err(err) if err.is::<nebula_client::Closed>() => return Err(Refusal::down()),
        Err(err) => return Err(conflict(sentence(&format!("{err:#}")))),
    }
    let now = nebula_core::clock::now_ms();
    app.refresher.patch(|f| {
        f.focus.asked = Some(Asked { at: now, auto });
        if auto {
            f.focus.runs.push(now);
        }
        f.focus.save(&app.focus, now);
    });
    Ok(StatusCode::ACCEPTED)
}

/// `POST /api/focus/feedback`: Not today or Wrong on one of the
/// orchestrator's picks, with an optional reason, kept for the next rank
/// to read. The pick leaves the focus.
pub(crate) async fn feedback_route(
    State(app): State<App>,
    got: Result<Json<api::FocusFeedback>, JsonRejection>,
) -> Result<StatusCode, Refusal> {
    let said = crate::body(got)?;
    let live = app.hub.live().ok_or_else(Refusal::down)?;
    let focus = &live.state.focus;
    let pick = focus
        .picks
        .iter()
        .find(|p| p.key == said.key)
        .filter(|_| focus.source == api::FocusSource::Orchestrator)
        .ok_or_else(|| {
            Refusal::new(
                StatusCode::NOT_FOUND,
                format!("{} is not among the orchestrator's picks.", said.key),
            )
        })?;
    let reason = said
        .reason
        .as_deref()
        .map(one_line)
        .filter(|r| !r.is_empty());
    if reason
        .as_ref()
        .is_some_and(|r| r.chars().count() > REASON_MAX)
    {
        return Err(Refusal::new(
            StatusCode::BAD_REQUEST,
            format!("The reason is over {REASON_MAX} characters."),
        ));
    }
    let now = nebula_core::clock::now_ms();
    let board = &live.state.board;
    let found = candidates(
        &live.state.sessions,
        &board.sections,
        &board.lanes,
        &HashSet::new(),
    );
    let what = found
        .iter()
        .find(|c| c.key == pick.key)
        .map_or_else(|| pick.key.clone(), Found::what);
    let feedback = Feedback {
        key: pick.key.clone(),
        verdict: said.verdict,
        reason,
        at: now,
        picked: pick.reason.clone(),
        what,
    };
    app.refresher.patch(|f| {
        f.focus.said.push(feedback);
        f.focus.save(&app.focus, now);
    });
    Ok(StatusCode::ACCEPTED)
}

/// `nebula orchestrator attention --json`: print what `nebula serve`
/// answers, on one line.
pub fn run_attention() -> anyhow::Result<()> {
    println!("{}", call_serve("GET", "/api/attention", b"")?);
    Ok(())
}

/// `nebula orchestrator picks set --json <file>`: set the picks the JSON in
/// `file` holds, `-` for stdin, and say what was set.
pub fn run_picks_set(file: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    use std::io::Read;
    let mut json = Vec::new();
    if file == "-" {
        std::io::stdin()
            .read_to_end(&mut json)
            .context("read the picks from stdin")?;
    } else {
        json = std::fs::read(file).with_context(|| format!("read the picks from {file}"))?;
    }
    let answer = call_serve("POST", "/api/focus/picks", &json)?;
    let set: serde_json::Value = serde_json::from_str(&answer).unwrap_or_default();
    println!("{}", set["said"].as_str().unwrap_or(&answer));
    Ok(())
}

/// What `nebula serve` answers the orchestrator's `method` on `path`, or
/// why it did not: it is not running, or it refused, as it says.
fn call_serve(method: &str, path: &str, body: &[u8]) -> anyhow::Result<String> {
    let port = nebula_client::serve::serving_port(&nebula_core::paths::serve_port_path())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "nebula serve is not running, and it holds the candidates the focus picks are \
                 ranked from: start it with `nebula serve --no-open`, or Shift+W in the TUI"
            )
        })?;
    let agent = nebula_core::env::non_empty(nebula_core::env::AGENT_ID).unwrap_or_default();
    let (status, answer) = nebula_client::serve::call(
        port,
        method,
        path,
        &[(AGENT_HEADER, &agent), ("Content-Type", "application/json")],
        body,
        READ_WAIT + std::time::Duration::from_secs(10),
    )
    .map_err(|err| anyhow::anyhow!("nebula serve did not answer: {err}"))?;
    if (200..300).contains(&status) {
        return Ok(answer);
    }
    let said: serde_json::Value = serde_json::from_str(&answer).unwrap_or_default();
    anyhow::bail!(
        "{}",
        said["error"]
            .as_str()
            .unwrap_or(&answer)
            .trim_end_matches('.')
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{BoardRow, Harness, Issue, Role, RowKind};

    pub(crate) fn session(id: &str, status: SessionStatus, issue: Option<&str>) -> Session {
        Session {
            id: id.into(),
            name: format!("{id} name"),
            project_id: "p".into(),
            project_name: "api".into(),
            branch: "main".into(),
            root_checkout: false,
            harness: Harness::Claude,
            role: Role::Worker,
            status,
            status_changed_at: 0,
            unseen: false,
            why: Some(format!("{id} why")),
            report_word: None,
            issue_url: None,
            issue_key: None,
            issue: issue.map(|key| self::issue(key, "started")),
            pr_url: None,
            pr_state: None,
        }
    }

    fn issue(key: &str, state_type: &str) -> Issue {
        Issue::from(&crate::refresh::tests::issue(key, state_type, 2, 0))
    }

    fn section(section: SectionName, ids: &[&str]) -> BoardSection {
        BoardSection {
            section,
            rows: ids
                .iter()
                .map(|id| BoardRow {
                    session_id: id.to_string(),
                    kind: RowKind::Session,
                    workers: 0,
                })
                .collect(),
        }
    }

    /// A board: `ask` needs you on ACME-1, which `run` works on too; `done`
    /// is done with no issue; `run` runs; ACME-2 is in flight and ACME-3 todo.
    fn board() -> (Vec<Session>, Vec<BoardSection>, Vec<IssueLane>) {
        let sessions = vec![
            session("ask", SessionStatus::NeedsFeedback, Some("ACME-1")),
            session("done", SessionStatus::Finished, None),
            session("run", SessionStatus::Running, Some("ACME-1")),
        ];
        let sections = vec![
            section(SectionName::NeedsYou, &["ask"]),
            section(SectionName::Done, &["done"]),
            section(SectionName::Running, &["run"]),
        ];
        let lanes = vec![
            IssueLane {
                lane: IssueLaneName::InFlight,
                issues: vec![issue("ACME-2", "started")],
            },
            IssueLane {
                lane: IssueLaneName::Todo,
                issues: vec![issue("ACME-3", "unstarted")],
            },
        ];
        (sessions, sections, lanes)
    }

    fn keys(found: &[Found]) -> Vec<(String, Lane)> {
        found.iter().map(|f| (f.key.clone(), f.lane)).collect()
    }

    /// The candidates are what needs you and what is done, by the issue
    /// joined to them or by session, then the issues in the lanes; a
    /// running session is none, and a key the user said not today to is
    /// left out.
    #[test]
    fn the_candidates_are_the_rows_that_need_you_or_are_done_then_the_lanes() {
        let (sessions, sections, lanes) = board();
        let found = candidates(&sessions, &sections, &lanes, &HashSet::new());
        assert_eq!(
            keys(&found),
            [
                ("ACME-1".into(), Lane::NeedsYou),
                ("done".into(), Lane::Done),
                ("ACME-2".into(), Lane::InFlight),
                ("ACME-3".into(), Lane::Todo),
            ]
        );
        let on: Vec<&str> = found[0].sessions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(on, ["ask", "run"], "the row's session first");
        assert_eq!(found[0].what(), "ACME-1 ACME-1 title");
        assert_eq!(found[1].what(), "done name");
        let off = HashSet::from(["ACME-2".to_string(), "done".to_string()]);
        assert_eq!(
            keys(&candidates(&sessions, &sections, &lanes, &off)),
            [
                ("ACME-1".into(), Lane::NeedsYou),
                ("ACME-3".into(), Lane::Todo)
            ]
        );
    }

    fn picks(at: i64, keys: &[&str], ranked: &[Found]) -> Kept {
        Kept {
            picks: Some(Picks {
                at,
                picks: keys
                    .iter()
                    .map(|k| Pick {
                        key: k.to_string(),
                        reason: format!("{k} first"),
                    })
                    .collect(),
                ranked: seen(ranked),
            }),
            ..Kept::default()
        }
    }

    const HOUR: i64 = 3600 * 1000;

    fn shown_keys(shown: Shown) -> Result<Vec<String>, Option<FocusFallback>> {
        match shown {
            Shown::Orchestrator(_, picks) => Ok(picks.iter().map(|p| p.key.clone()).collect()),
            Shown::Rule(why) => Err(why),
        }
    }

    /// Picks are shown while they are candidates; over three hours old
    /// with the candidates changed since they are stale, and not before;
    /// none on the board now, or each put aside by the user, and the rule
    /// stands in, saying why. With no picks ever, the rule stands in and
    /// says nothing.
    #[test]
    fn the_picks_stand_until_stale_gone_or_set_aside() {
        let (sessions, sections, lanes) = board();
        let found = candidates(&sessions, &sections, &lanes, &HashSet::new());
        let now = 100 * HOUR;
        assert_eq!(
            shown_keys(shown(&Kept::default(), &found, true, now)),
            Err(None)
        );

        let kept = picks(now - 4 * HOUR, &["ACME-3", "done"], &found);
        assert_eq!(
            shown_keys(shown(&kept, &found, true, now)),
            Ok(vec!["ACME-3".into(), "done".into()]),
            "old, but nothing changed"
        );
        let fewer = &found[1..];
        assert_eq!(
            shown_keys(shown(&kept, fewer, true, now)),
            Err(Some(FocusFallback::Stale))
        );
        let young = picks(now - 2 * HOUR, &["ACME-3", "ACME-1"], &found);
        assert_eq!(
            shown_keys(shown(&young, fewer, true, now)),
            Ok(vec!["ACME-3".into()]),
            "young: one left the board, the other stands"
        );
        let gone = picks(now - HOUR, &["ACME-1"], &found);
        assert_eq!(
            shown_keys(shown(&gone, fewer, true, now)),
            Err(Some(FocusFallback::Gone))
        );
        assert_eq!(
            shown_keys(shown(&young, &found, false, now)),
            Err(Some(FocusFallback::Unread)),
            "issues not read yet, which picks are keyed by"
        );
        let nothing = picks(now - HOUR, &[], &found);
        assert_eq!(shown_keys(shown(&nothing, &found, true, now)), Ok(vec![]));

        let mut said = picks(now - HOUR, &["ACME-3", "done"], &found);
        let feedback = |key: &str, at| Feedback {
            key: key.into(),
            verdict: Verdict::Wrong,
            reason: None,
            at,
            picked: String::new(),
            what: String::new(),
        };
        said.said.push(feedback("ACME-3", now - 2 * HOUR));
        assert_eq!(
            shown_keys(shown(&said, &found, true, now)),
            Ok(vec!["ACME-3".into(), "done".into()]),
            "said of the picks before these"
        );
        said.said.push(feedback("ACME-3", now));
        assert_eq!(
            shown_keys(shown(&said, &found, true, now)),
            Ok(vec!["done".into()])
        );
        said.said.push(feedback("done", now));
        assert_eq!(
            shown_keys(shown(&said, &found, true, now)),
            Err(Some(FocusFallback::SetAside))
        );
    }

    /// The page's own ask is due with no picks yet, and for stale ones,
    /// four times a day at most; never for picks that stand.
    #[test]
    fn a_rank_is_due_for_stale_picks_four_times_a_day() {
        let (sessions, sections, lanes) = board();
        let found = candidates(&sessions, &sections, &lanes, &HashSet::new());
        let now = day_start(nebula_core::clock::now_ms()) + 20 * HOUR;
        assert!(due(&Kept::default(), &found, now), "never ranked");
        let old = picks(now - 4 * HOUR, &["ACME-1"], &found);
        assert!(!due(&old, &found, now), "old, nothing changed");
        assert!(due(&old, &found[1..], now), "old, and changed");
        let young = picks(now - 2 * HOUR, &["ACME-1"], &found);
        assert!(!due(&young, &found[1..], now), "changed, under three hours");
        let mut four = old.clone();
        four.runs = vec![now - 19 * HOUR, now - 5 * HOUR, now - HOUR, now - 1];
        assert!(!due(&four, &found[1..], now), "four today already");
        four.runs[0] = now - 21 * HOUR;
        assert!(due(&four, &found[1..], now), "one of them yesterday");
    }

    /// A rank asked for is held until the daemon types it, at work once it
    /// is, ended when the orchestrator is at rest after it with no picks
    /// set, and over once picks come; one the daemon never heard of goes
    /// after a grace.
    #[test]
    fn a_rank_is_held_then_ranking_then_ended_or_done() {
        use nebula_core::RankTurn;
        let asked = Kept {
            asked: Some(Asked {
                at: 100,
                auto: true,
            }),
            ..Kept::default()
        };
        let state = |kept: &Kept, held, typed, orchestrator, now| {
            rank(
                kept,
                RankTurn {
                    held_at: held,
                    typed_at: typed,
                },
                orchestrator,
                now,
            )
            .map(|r| r.state)
        };
        let resting = Some((SessionStatus::Finished, 50));
        assert_eq!(state(&Kept::default(), Some(1), None, resting, 200), None);
        assert_eq!(
            state(&asked, None, None, resting, 200),
            Some(RankState::Held),
            "not heard of yet"
        );
        assert_eq!(state(&asked, None, None, resting, 100 + ASK_GRACE_MS), None);
        assert_eq!(
            state(&asked, Some(100), Some(90), resting, 200),
            Some(RankState::Held)
        );
        assert_eq!(
            state(&asked, None, Some(90), resting, 200),
            Some(RankState::Held),
            "typed before this ask"
        );
        assert_eq!(
            state(&asked, None, Some(150), resting, 200),
            Some(RankState::Ranking),
            "typed, its status not moved on yet"
        );
        let working = Some((SessionStatus::Running, 160));
        assert_eq!(
            state(&asked, None, Some(150), working, 200),
            Some(RankState::Ranking)
        );
        let after = Some((SessionStatus::Finished, 170));
        assert_eq!(
            state(&asked, None, Some(150), after, 200),
            Some(RankState::Ended)
        );
        let mut done = asked.clone();
        done.picks = Some(Picks {
            at: 165,
            picks: Vec::new(),
            ranked: Vec::new(),
        });
        assert_eq!(state(&done, None, Some(150), after, 200), None);
        let r = rank(&asked, RankTurn::default(), resting, 200).unwrap();
        assert!(r.auto);
        assert_eq!(r.at, 100);
    }

    /// Picks are refused past three, twice, with no reason or too long a
    /// one, and for a key that is no candidate, or one the user said not
    /// today to; a key attention printed that has left the board since is
    /// taken, and each reason goes on one line.
    #[test]
    fn picks_are_held_to_the_candidates() {
        let (sessions, sections, lanes) = board();
        let found = candidates(&sessions, &sections, &lanes, &HashSet::new());
        let mut kept = Kept::default();
        let check = |raw: &str, kept: &Kept, off: &HashSet<String>| {
            validate(raw.as_bytes(), kept, &found, off)
        };
        let none = HashSet::new();
        let pick = |key: &str| format!(r#"{{"key": "{key}", "reason": "it waits"}}"#);
        let list = |keys: &[&str]| {
            format!(
                "[{}]",
                keys.iter().map(|k| pick(k)).collect::<Vec<_>>().join(",")
            )
        };

        let set = check(
            r#"[{"key": " ACME-1 ", "reason": "Unblocks\n  the release"}]"#,
            &kept,
            &none,
        )
        .unwrap();
        assert_eq!(
            set,
            [Pick {
                key: "ACME-1".into(),
                reason: "Unblocks the release".into()
            }]
        );
        assert_eq!(check("[]", &kept, &none).unwrap(), []);
        let err = check(&list(&["ACME-1", "ACME-2", "ACME-3", "done"]), &kept, &none).unwrap_err();
        assert_eq!(err, "At most three picks, not 4.");
        let err = check(&list(&["ACME-1", "ACME-1"]), &kept, &none).unwrap_err();
        assert_eq!(err, "ACME-1 is picked twice.");
        let err = check(&list(&["ACME-9"]), &kept, &none).unwrap_err();
        assert!(
            err.starts_with("ACME-9 is no candidate: pick keys as"),
            "{err}"
        );
        let err = check(&list(&["ACME-2"]), &kept, &HashSet::from(["ACME-2".into()])).unwrap_err();
        assert_eq!(
            err,
            "ACME-2 is no candidate today: the user said not today to it."
        );
        let err = check(r#"[{"key": "ACME-1", "reason": " "}]"#, &kept, &none).unwrap_err();
        assert_eq!(err, "The pick of ACME-1 has no reason.");
        let long = format!(r#"[{{"key": "ACME-1", "reason": "{}"}}]"#, "a".repeat(301));
        let err = check(&long, &kept, &none).unwrap_err();
        assert_eq!(err, "The reason for ACME-1 is over 300 characters.");
        let err = check(r#"{"key": "ACME-1"}"#, &kept, &none).unwrap_err();
        assert!(
            err.starts_with("The picks are not a JSON array of {\"key\", \"reason\"}: "),
            "{err}"
        );

        kept.seen = vec![Seen {
            key: "ACME-9".into(),
            lane: Lane::Todo,
        }];
        assert_eq!(check(&list(&["ACME-9"]), &kept, &none).unwrap().len(), 1);
    }

    /// Attention is compact: the candidates with what matters to rank them,
    /// nothing empty, and what the user said of picks in the last week,
    /// newest first, which rides along to the next rank.
    #[test]
    fn attention_carries_the_candidates_and_what_the_user_said() {
        let (mut sessions, sections, mut lanes) = board();
        sessions[0].pr_state = Some(api::PrState::Draft);
        sessions[0].status_changed_at = -5 * 60 * 1000;
        lanes[1].issues[0].started = Some(api::IssueStart {
            project_id: "p".into(),
            project_name: "api".into(),
            captain_id: "c".into(),
            at: 0,
            held: true,
            move_failed: None,
        });
        let found = candidates(&sessions, &sections, &lanes, &HashSet::new());
        let now = 0;
        let mut kept = Kept::default();
        let said = |key: &str, verdict, at| Feedback {
            key: key.into(),
            verdict,
            reason: Some("waits on design".into()),
            at,
            picked: "It is urgent".into(),
            what: format!("{key} title"),
        };
        kept.said = vec![
            said("ACME-7", Verdict::Wrong, now - 8 * 24 * HOUR),
            said("ACME-8", Verdict::NotToday, now - 2 * 24 * HOUR),
            said("ACME-9", Verdict::Wrong, now - HOUR),
        ];
        let refresh = api::Refresh {
            running: false,
            at: Some(now - 4 * 60 * 1000),
            problems: Vec::new(),
        };
        let json = serde_json::to_value(attention(&found, &kept, &refresh, now)).unwrap();
        assert_eq!(json["read"], "4m");
        assert!(json.get("problems").is_none() && json.get("more").is_none());
        assert_eq!(
            json["candidates"][0],
            serde_json::json!({
                "key": "ACME-1",
                "lane": "needsYou",
                "issue": {"title": "ACME-1 title", "state": "started", "priority": "high", "changed": "now"},
                "sessions": [
                    {"id": "ask", "project": "api", "name": "ask name", "status": "needsFeedback", "why": "ask why", "since": "5m", "pr": "draft"},
                    {"id": "run", "project": "api", "name": "run name", "status": "running", "why": "run why", "since": "now"},
                ],
            })
        );
        assert_eq!(json["candidates"][1]["key"], "done");
        assert!(json["candidates"][1].get("issue").is_none());
        assert_eq!(json["candidates"][3]["issue"]["handed"], "api");
        assert!(json["candidates"][3].get("sessions").is_none());
        let said: Vec<&str> = json["feedback"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["key"].as_str().unwrap())
            .collect();
        assert_eq!(said, ["ACME-9", "ACME-8"], "the last week's, newest first");
        assert_eq!(
            json["feedback"][1],
            serde_json::json!({"key": "ACME-8", "said": "notToday", "reason": "waits on design", "picked": "It is urgent", "what": "ACME-8 title", "ago": "2d"})
        );
    }

    /// The file keeps the picks, what was said and the page's own ranks of
    /// today, dropping what is too old, and nothing kept in memory.
    #[test]
    fn what_the_focus_keeps_outlasts_the_serve_in_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("focus.json");
        assert_eq!(Kept::load(&file), Kept::default());
        let now = nebula_core::clock::now_ms();
        let said = |at| Feedback {
            key: "ACME-1".into(),
            verdict: Verdict::NotToday,
            reason: None,
            at,
            picked: "p".into(),
            what: "w".into(),
        };
        let mut kept = Kept {
            picks: Some(Picks {
                at: now,
                picks: vec![Pick {
                    key: "ACME-1".into(),
                    reason: "r".into(),
                }],
                ranked: vec![Seen {
                    key: "ACME-1".into(),
                    lane: Lane::Todo,
                }],
            }),
            said: vec![said(now - 15 * 24 * HOUR), said(now)],
            runs: vec![day_start(now) - 1, now],
            seen: vec![Seen {
                key: "x".into(),
                lane: Lane::Done,
            }],
            asked: Some(Asked {
                at: now,
                auto: false,
            }),
        };
        kept.save(&file, now);
        let read = Kept::load(&file);
        assert_eq!(read.picks, kept.picks);
        assert_eq!(read.said, [said(now)]);
        assert_eq!(read.runs, [now]);
        assert!(read.seen.is_empty() && read.asked.is_none());
        std::fs::write(&file, "{").unwrap();
        assert_eq!(Kept::load(&file), Kept::default());
    }

    /// Not today lasts until the day ends on this machine's clock.
    #[test]
    fn not_today_lasts_the_day() {
        let now = nebula_core::clock::now_ms();
        let start = day_start(now);
        assert!(start <= now && now - start < 25 * HOUR, "{start} {now}");
        assert_eq!(day_start(start), start);
        let kept = Kept {
            said: vec![
                Feedback {
                    key: "ACME-1".into(),
                    verdict: Verdict::NotToday,
                    reason: None,
                    at: start,
                    picked: String::new(),
                    what: String::new(),
                },
                Feedback {
                    key: "ACME-2".into(),
                    verdict: Verdict::NotToday,
                    reason: None,
                    at: start - 1,
                    picked: String::new(),
                    what: String::new(),
                },
                Feedback {
                    key: "ACME-3".into(),
                    verdict: Verdict::Wrong,
                    reason: None,
                    at: now,
                    picked: String::new(),
                    what: String::new(),
                },
            ],
            ..Kept::default()
        };
        assert_eq!(off(&kept, now), HashSet::from(["ACME-1".to_string()]));
    }
}
