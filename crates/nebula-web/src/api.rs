//! The JSON `nebula serve` answers with, mirroring the web app's contract
//! (`app/src/api/contract.ts`) type for type, and the STATE built from the
//! entity tree, the BOARD and the last REFRESH.

use crate::focus;
use crate::refresh::{self, Fetched};
use nebula_client::board::{self, BoardRead, Section};
use nebula_client::Tree;
use nebula_core::{Agent, AgentId, AgentKind, AgentRole, AgentStatus};
use nebula_forge::issues::IssueSource;
use nebula_forge::linear::Assigned;
use nebula_forge::pull_request::Standing;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionStatus {
    Fresh,
    Running,
    Finished,
    NeedsFeedback,
    Terminated,
    Disconnected,
}

impl From<AgentStatus> for SessionStatus {
    fn from(status: AgentStatus) -> Self {
        match status {
            AgentStatus::Fresh => SessionStatus::Fresh,
            AgentStatus::Running => SessionStatus::Running,
            AgentStatus::Finished => SessionStatus::Finished,
            AgentStatus::NeedsFeedback => SessionStatus::NeedsFeedback,
            AgentStatus::Terminated => SessionStatus::Terminated,
            AgentStatus::Disconnected => SessionStatus::Disconnected,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Harness {
    Claude,
    Codex,
    Cursor,
    Pi,
    Muse,
    Grok,
    OpenCode,
    Custom,
}

impl From<AgentKind> for Harness {
    fn from(kind: AgentKind) -> Self {
        match kind {
            AgentKind::Claude => Harness::Claude,
            AgentKind::Codex => Harness::Codex,
            AgentKind::Cursor => Harness::Cursor,
            AgentKind::Pi => Harness::Pi,
            AgentKind::Muse => Harness::Muse,
            AgentKind::Grok => Harness::Grok,
            AgentKind::OpenCode => Harness::OpenCode,
            AgentKind::Custom => Harness::Custom,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Worker,
    Orchestrator,
    Captain,
}

impl From<AgentRole> for Role {
    fn from(role: AgentRole) -> Self {
        match role {
            AgentRole::Worker => Role::Worker,
            AgentRole::Orchestrator => Role::Orchestrator,
            AgentRole::Captain => Role::Captain,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub space: Option<String>,
    /// The Linear team it reads its issues from, by its `issues` setting.
    pub team: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub name: String,
    pub project_id: String,
    pub project_name: String,
    pub branch: String,
    pub root_checkout: bool,
    pub harness: Harness,
    pub role: Role,
    pub status: SessionStatus,
    pub status_changed_at: i64,
    pub unseen: bool,
    pub why: Option<String>,
    pub report_word: Option<&'static str>,
    pub issue_url: Option<String>,
    pub issue_key: Option<String>,
    /// The assigned Linear issue it works on, as the last refresh read it.
    pub issue: Option<Issue>,
    /// The pull request on its checkout, as the last refresh read it.
    pub pr_url: Option<String>,
    pub pr_state: Option<PrState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PrState {
    Open,
    Draft,
    Merged,
    Closed,
}

impl From<Standing> for PrState {
    fn from(standing: Standing) -> Self {
        match standing {
            Standing::Open => PrState::Open,
            Standing::Draft => PrState::Draft,
            Standing::Merged => PrState::Merged,
            Standing::Closed => PrState::Closed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub key: String,
    pub url: String,
    pub title: String,
    pub priority: u8,
    pub state: String,
    pub state_type: String,
    pub assignee: Option<String>,
    pub branch_name: String,
    pub updated_at: i64,
    pub team: String,
    pub project: Option<String>,
    /// When Start handed it to a captain, and to which.
    pub started: Option<IssueStart>,
    /// The project Start last handed an issue of its Linear project to.
    pub last_project: Option<String>,
}

/// An issue Start handed to a project's CAPTAIN.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueStart {
    pub project_id: String,
    pub project_name: String,
    pub captain_id: String,
    pub at: i64,
    /// Its brief waits for the captain to be at rest with an empty input
    /// box, as the daemon says now.
    pub held: bool,
    /// Why Linear was not moved to its started state, if it was not.
    pub move_failed: Option<String>,
}

/// The body of `POST /api/issues/:key/start`.
#[derive(Debug, Clone, Deserialize)]
pub struct StartIssue {
    pub project: String,
    /// Start an issue handed to a captain already, again.
    #[serde(default)]
    pub again: bool,
}

/// What a Start did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueStarted {
    pub captain_id: String,
    pub held: bool,
    /// The Linear state it was moved to; null when it was started already
    /// or the move failed.
    pub moved_to: Option<String>,
    pub move_failed: Option<String>,
}

impl From<&Assigned> for Issue {
    fn from(i: &Assigned) -> Self {
        Issue {
            key: i.key.clone(),
            url: i.url.clone(),
            title: i.title.clone(),
            priority: i.priority,
            state: i.state.clone(),
            state_type: i.state_type.clone(),
            assignee: Some(i.assignee.clone()).filter(|a| !a.is_empty()),
            branch_name: i.branch_name.clone(),
            updated_at: i.updated_at,
            team: i.team.clone(),
            project: i.project.clone(),
            started: None,
            last_project: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum IssueLaneName {
    InFlight,
    Todo,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueLane {
    pub lane: IssueLaneName,
    pub issues: Vec<Issue>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Refresh {
    pub running: bool,
    pub at: Option<i64>,
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SectionName {
    NeedsYou,
    Done,
    Running,
    Captains,
}

impl From<Section> for SectionName {
    fn from(section: Section) -> Self {
        match section {
            Section::NeedsYou => SectionName::NeedsYou,
            Section::Done => SectionName::Done,
            Section::Running => SectionName::Running,
            Section::Captains => SectionName::Captains,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RowKind {
    Session,
    Captain,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardRow {
    pub session_id: String,
    pub kind: RowKind,
    pub workers: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardSection {
    pub section: SectionName,
    pub rows: Vec<BoardRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    pub sections: Vec<BoardSection>,
    pub lanes: Vec<IssueLane>,
    pub idle: usize,
    pub stopped: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FocusSource {
    Rule,
    Orchestrator,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusPick {
    /// The candidate's key: its issue's when one is joined to it, else its
    /// session's id.
    pub key: String,
    /// The session the pick opens; none for an issue no session works on.
    pub session_id: Option<String>,
    pub reason: String,
    pub answers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Focus {
    pub source: FocusSource,
    /// When the orchestrator set the picks shown, or the ones the rule
    /// stands in for; none while it never has.
    pub at: Option<i64>,
    pub picks: Vec<FocusPick>,
    /// Why the rule stands in for the orchestrator's picks.
    pub fallback: Option<FocusFallback>,
    /// A rank asked of the orchestrator and not done.
    pub rank: Option<FocusRank>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FocusFallback {
    /// Over three hours old, the candidates changed since.
    Stale,
    /// None of them is a candidate now.
    Gone,
    /// The user put each one aside with Not today or Wrong.
    SetAside,
    /// Linear and the pull requests, which they are keyed by, are not read
    /// yet.
    Unread,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusRank {
    pub at: i64,
    pub auto: bool,
    pub state: RankState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RankState {
    /// Held until the orchestrator is at rest with an empty input box.
    Held,
    /// Typed into it, and it is at work on it.
    Ranking,
    /// Its turn ended without setting picks.
    Ended,
}

/// The body of `POST /api/focus/rank`.
#[derive(Debug, Clone, Deserialize)]
pub struct RankFocus {
    /// The page asks on opening, not Re-rank: only when the picks are due.
    #[serde(default)]
    pub auto: bool,
}

/// What the user said of one of the orchestrator's picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    NotToday,
    Wrong,
}

/// The body of `POST /api/focus/feedback`.
#[derive(Debug, Clone, Deserialize)]
pub struct FocusFeedback {
    pub key: String,
    pub verdict: Verdict,
    #[serde(default)]
    pub reason: Option<String>,
}

/// The answer to `POST /api/focus/picks`.
#[derive(Debug, Clone, Serialize)]
pub struct PicksSet {
    pub said: String,
}

/// What `GET /api/attention` answers, `nebula orchestrator attention
/// --json`: the candidates for the focus picks, compact, since every rank
/// reads them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attention {
    /// How long ago Linear and the pull requests were read (`4m`).
    pub read: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<String>,
    pub candidates: Vec<Candidate>,
    /// How many candidates there are past these.
    #[serde(skip_serializing_if = "is_zero")]
    pub more: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub feedback: Vec<Said>,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Lane {
    NeedsYou,
    Done,
    InFlight,
    Todo,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub key: String,
    pub lane: Lane,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<CandidateIssue>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sessions: Vec<CandidateSession>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateIssue {
    pub title: String,
    pub state: String,
    /// `urgent`, `high`, `medium` or `low`; none for no priority.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<&'static str>,
    /// Its Linear project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// How long since it last changed.
    pub changed: String,
    /// The project whose captain Start handed it to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handed: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateSession {
    pub id: String,
    pub project: String,
    pub name: String,
    pub status: SessionStatus,
    /// The line the board reads under its row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// How long it has been in its status.
    pub since: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr: Option<PrState>,
}

/// What the user said of a recent pick.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Said {
    pub key: String,
    pub said: Verdict,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The pick's reason.
    pub picked: String,
    /// What the pick was: its issue's title, else its session's name.
    pub what: String,
    pub ago: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub version: &'static str,
    pub protocol: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TuiState {
    pub session_id: Option<String>,
}

/// The devices paired to reach the page over the tailnet, and the pairing
/// open for the next, as the Mac's page shows them.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Devices {
    /// This machine's tailnet name: none while Tailscale is not running.
    pub tailnet: Option<String>,
    pub devices: Vec<Device>,
    pub pairing: Option<Pairing>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    /// The Tailscale user who paired it, the one it is let in as.
    pub login: String,
    pub paired_at: i64,
    pub last_used: i64,
    /// It turned notifications on: it keeps a push subscription.
    pub push: bool,
}

/// What a paired device's page turns notifications on with: this machine's
/// VAPID public key, base64url, its `applicationServerKey`, and whether this
/// device keeps a subscription.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Push {
    pub key: String,
    pub on: bool,
}

/// A browser's push subscription, as `PushSubscription.toJSON()` has it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushSubscription {
    pub endpoint: String,
    pub keys: PushKeys,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushKeys {
    pub p256dh: String,
    pub auth: String,
}

/// What came of a test notification.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushTested {
    pub said: String,
}

/// A one-time code that pairs a device until `expires_at`, the link that
/// carries it, and that link's QR code, a row of `1` (dark) and `0` for
/// each line of modules.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pairing {
    pub code: String,
    pub url: String,
    pub qr: Vec<String>,
    pub expires_at: i64,
}

/// The `202` of a show: whether the TUI touched last had the session on
/// screen already, where putting it up changes nothing to see.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shown {
    pub already_on_screen: bool,
}

impl Shown {
    pub fn of(tui: &TuiState, id: &str) -> Self {
        Shown {
            already_on_screen: tui.session_id.as_deref() == Some(id),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub projects: Vec<Project>,
    pub sessions: Vec<Session>,
    pub board: Board,
    pub focus: Focus,
    pub orchestrator_id: Option<String>,
    /// What the TUIs show: none while no TUI is open, as a daemon too old
    /// to say has it.
    pub tui: Option<TuiState>,
    pub refresh: Refresh,
    /// The day the file `today_file` names, as last read: none while that
    /// is not set, or the file is missing or unreadable.
    pub today: Option<Today>,
    /// Why that file could not be read as a day, when it could not.
    pub today_problem: Option<String>,
    pub server: Server,
}

/// A day as a program such as agentos writes it to the file `today_file`
/// names, the contract's `Today`: read from the file as it stands, beside
/// its `version`, and served as read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Today {
    pub at: i64,
    pub todos: TodayTodos,
    pub training: Option<TodayTraining>,
    pub quiet: Vec<TodayQuiet>,
    pub nightly: Option<TodayNightly>,
    pub off: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayTodos {
    pub count: u32,
    pub overdue: u32,
    pub top: Vec<TodayTodo>,
    pub url: Option<String>,
    pub problem: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayTodo {
    pub title: String,
    pub due: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayTraining {
    pub block: Option<String>,
    pub today: Option<String>,
    pub url: Option<String>,
    pub lifts: u32,
    pub lifts_per_week: u32,
    /// Why the training could not be read: none in an older file.
    pub problem: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayQuiet {
    pub name: String,
    pub brain: String,
    pub days: u32,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayNightly {
    pub at: i64,
    pub ok: bool,
    pub why: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogOption {
    pub number: u32,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dialog {
    pub question: String,
    pub options: Vec<DialogOption>,
}

impl From<nebula_core::dialog::Choice> for Dialog {
    fn from(choice: nebula_core::dialog::Choice) -> Self {
        Dialog {
            question: choice.question,
            options: choice
                .options
                .into_iter()
                .map(|(number, label)| DialogOption { number, label })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReplySource {
    Transcript,
    Screen,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    pub text: String,
    pub source: ReplySource,
    pub at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub session: Session,
    pub reply: Option<Reply>,
    pub answers: Vec<String>,
    pub dialog: Option<Dialog>,
    pub worker_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Screen {
    pub rows: Vec<String>,
    pub cols: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillCli {
    Claude,
    Codex,
    Cursor,
}

impl SkillCli {
    pub fn title(self) -> &'static str {
        match self {
            SkillCli::Claude => "Claude",
            SkillCli::Codex => "Codex",
            SkillCli::Cursor => "Cursor",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillScope {
    Managed,
    Personal,
    Project,
    Nested,
    AddedDir,
    Plugin,
    ClaudeAi,
    Admin,
    System,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillLoad {
    pub cli: SkillCli,
    pub name: String,
    pub scope: SkillScope,
    pub path: String,
    pub project: Option<String>,
    pub space: Option<String>,
    pub shadowed_by: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillState {
    Off,
    NameOnly,
    UserInvocableOnly,
}

/// Where a switch the page sets stands: on, one of Claude's two in
/// between, or off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillSetting {
    On,
    NameOnly,
    UserInvocableOnly,
    Off,
}

impl From<SkillState> for SkillSetting {
    fn from(state: SkillState) -> Self {
        match state {
            SkillState::Off => SkillSetting::Off,
            SkillState::NameOnly => SkillSetting::NameOnly,
            SkillState::UserInvocableOnly => SkillSetting::UserInvocableOnly,
        }
    }
}

/// A switch the page can set for one CLI, and where it stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillControl {
    pub cli: SkillCli,
    pub setting: &'static str,
    pub file: String,
    pub state: SkillSetting,
    pub plugin: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSwitch {
    pub cli: SkillCli,
    pub state: SkillState,
    pub setting: &'static str,
    pub file: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillProblemKind {
    BrokenLink,
    LinkOutside,
    Unreadable,
    BadFrontmatter,
    NameMismatch,
    BadName,
    LongDescription,
    MissingField,
    DuplicateName,
    OneCli,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillProblem {
    pub kind: SkillProblemKind,
    pub text: String,
    /// Part of a fix the user kept off the list.
    pub kept: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFix {
    pub id: String,
    pub kind: SkillProblemKind,
    pub title: String,
    pub place: String,
    pub text: String,
    pub fix: String,
    pub skills: Vec<String>,
    pub acts: Vec<SkillFixAct>,
}

/// What the page can do about a fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillAct {
    RemoveLinks,
    RemoveCommand,
    Keep,
    Unkeep,
    Compare,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFixAct {
    pub act: SkillAct,
    /// For a removal, what it does in git, said before it is done.
    pub says: Option<String>,
    /// Why the page will not do it.
    pub refused: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRepo {
    pub root: String,
    pub branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SkillSource {
    Repo,
    Folder,
    Installer {
        source: String,
        url: Option<String>,
        path: Option<String>,
    },
    Plugin {
        plugin: String,
        version: Option<String>,
        marketplace: String,
        from: Option<String>,
    },
    ClaudeAi,
    Bundled {
        cli: SkillCli,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillKind {
    Skill,
    Command,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub kind: SkillKind,
    pub path: String,
    pub source: SkillSource,
    pub repo: Option<GitRepo>,
    pub loads: Vec<SkillLoad>,
    pub switches: Vec<SkillSwitch>,
    pub controls: Vec<SkillControl>,
    pub problems: Vec<SkillProblem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skills<'a> {
    pub skills: Vec<&'a Skill>,
    pub fixes: &'a [SkillFix],
    pub kept: &'a [SkillFix],
    pub home: String,
    pub clis: Vec<SkillCli>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillUsage {
    pub days: u32,
    pub used: Vec<SkillUse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillUse {
    pub id: String,
    pub count: u32,
    pub last: i64,
    pub project: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillField {
    pub key: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFile {
    pub path: String,
    pub size: u64,
    pub link: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDetail {
    pub skill: Skill,
    pub file: Option<String>,
    pub frontmatter: Vec<SkillField>,
    pub frontmatter_error: Option<String>,
    pub body: Option<String>,
    pub unread: Option<String>,
    pub files: Vec<SkillFile>,
    pub truncated: bool,
    pub edit: Option<SkillEdit>,
    pub uneditable: Option<String>,
    pub delete: Option<SkillDelete>,
    pub undeletable: Option<String>,
    pub changes: Option<String>,
}

/// What deleting a skill moves to the Trash, said before it is done, and
/// the version a delete sends back.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDelete {
    pub goes: Vec<SkillGoes>,
    pub says: Vec<String>,
    pub note: Option<String>,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillGoes {
    pub path: String,
    pub link: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeleteSkill {
    pub id: String,
    pub version: String,
}

/// A skill's file as the page edits it: its text, and the version a save
/// sends back with the new text.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillEdit {
    pub text: String,
    pub version: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveSkill {
    pub id: String,
    pub version: String,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CheckSkill {
    pub id: String,
    pub text: String,
}

/// What the resolver says of a skill's file holding a text.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCheck {
    pub problems: Vec<SkillProblem>,
    pub body: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SwitchSkills {
    pub cli: SkillCli,
    pub set: Vec<SkillTo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillTo {
    pub id: String,
    pub state: SkillSetting,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SwitchPlugin {
    pub plugin: String,
    pub on: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillWrite {
    pub file: String,
    pub link: Option<String>,
    pub changed: bool,
    pub repo: Option<String>,
    pub before: Vec<SkillTo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FixSkills {
    pub fix: String,
    pub act: SkillAct,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UndoFix {
    pub undo: String,
}

/// What an act on a fix, or its Undo, did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDone {
    pub said: String,
    pub repos: Vec<String>,
    pub skipped: Vec<String>,
    pub undo: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCompare {
    pub files: Vec<SkillCompared>,
    pub same: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCompared {
    pub skill: String,
    pub file: Option<String>,
    pub text: Option<String>,
    pub unread: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    pub error: String,
}

/// The answers a focus pick's reply offers, read once per state of its
/// session (its status and when that began): `None` while the read is out.
pub struct PickAnswers {
    pub state: (SessionStatus, i64),
    pub answers: Option<Vec<String>>,
}

/// What `nebula serve` reads of `config.json`: each project's SPACE and
/// `issues`, each space's `issues`, and the file its Today band shows,
/// read as the TUI and the daemon read their settings
/// ([`nebula_core::settings`]).
#[derive(Default, Deserialize)]
#[serde(default)]
struct Settings {
    projects: BTreeMap<PathBuf, ProjectSettings>,
    spaces: BTreeMap<String, SpaceSettings>,
    today_file: String,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ProjectSettings {
    space: String,
    issues: String,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct SpaceSettings {
    issues: String,
}

fn settings() -> Settings {
    nebula_core::settings::load::<Settings>(
        &nebula_core::paths::config_path(),
        &nebula_core::paths::config_local_path(),
    )
    .value
}

/// `today_file` as set, untrimmed: empty when it is not.
pub fn today_file() -> String {
    settings().today_file
}

/// The SPACE each project is filed under, by repo path.
pub fn spaces() -> BTreeMap<PathBuf, String> {
    settings()
        .projects
        .into_iter()
        .filter_map(|(path, p)| Some((path, nebula_core::settings::space_label(&p.space)?)))
        .collect()
}

/// The Linear teams the projects at `roots` read their issues from
/// ([`teams`]).
pub fn linear_teams<'a>(roots: impl IntoIterator<Item = &'a Path>) -> BTreeSet<String> {
    teams(roots).into_values().collect()
}

/// The Linear team each project at `roots` reads its issues from, by repo
/// path, by the rule the TUI's ISSUES MODAL reads it by: a space's `issues`
/// over its projects' own. A project whose issues are GitHub's has none.
pub fn teams<'a>(roots: impl IntoIterator<Item = &'a Path>) -> BTreeMap<PathBuf, String> {
    let settings = settings();
    let spaces = || {
        settings
            .spaces
            .iter()
            .map(|(name, s)| (name.as_str(), s.issues.as_str()))
    };
    roots
        .into_iter()
        .filter_map(|root| {
            let own = settings.projects.get(root);
            let setting = nebula_forge::issues::project_setting(
                own.map_or("", |p| p.issues.as_str()),
                own.map_or("", |p| p.space.as_str()),
                spaces(),
            );
            match IssueSource::parse(setting)? {
                IssueSource::Linear { team } => Some((root.to_path_buf(), team)),
                IssueSource::GitHub => None,
            }
        })
        .collect()
}

/// The STATE of `tree`, with `reads` what the board read off each
/// session's screen, `answers` what the focus picks' replies offer,
/// `spaces` the SPACE of each project by repo path, `teams` the Linear
/// team each reads its issues from, and `fetched` what the last refresh
/// read and Start keeps, and `today` the day file as last read. The rule's
/// focus is the board's rows that need you, then the ones done, in
/// attention order: a running session or a captain at rest is never one,
/// and neither is an issue.
pub fn state(
    tree: &Tree,
    reads: &HashMap<AgentId, BoardRead>,
    answers: &HashMap<AgentId, PickAnswers>,
    spaces: &BTreeMap<PathBuf, String>,
    teams: &BTreeMap<PathBuf, String>,
    fetched: &Fetched,
    today: &crate::today::TodayFile,
) -> State {
    let orchestrator = tree.orchestrator_agent();
    let mut order = tree.attention_sessions();
    if let Some(o) = orchestrator.filter(|o| !order.contains(&o.id)) {
        order.push(o.id.clone());
    }
    let sessions: Vec<Session> = order
        .iter()
        .filter_map(|id| tree.agents.iter().find(|a| &a.id == id))
        .filter_map(|a| session(tree, a, reads, fetched))
        .collect();
    let joined: HashSet<&str> = sessions
        .iter()
        .filter_map(|s| Some(s.issue.as_ref()?.key.as_str()))
        .collect();
    // A lane's issue says what Start did with it and where it last went.
    let startable = |i: &Assigned| Issue {
        started: fetched.starts.handed.get(&i.key).map(|h| IssueStart {
            project_id: h.project_id.clone(),
            project_name: h.project_name.clone(),
            captain_id: h.captain_id.clone(),
            at: h.at,
            held: tree.held_briefs.iter().any(|b| b.issue_url == i.url),
            move_failed: h.move_failed.clone(),
        }),
        last_project: fetched
            .starts
            .last_project
            .get(&crate::start::linear_project(i))
            .cloned(),
        ..Issue::from(i)
    };
    let lanes: Vec<IssueLane> = refresh::lanes(&fetched.issues, &joined)
        .into_iter()
        .map(|(lane, issues)| IssueLane {
            lane,
            issues: issues.into_iter().map(startable).collect(),
        })
        .collect();
    let built = board::Board::build(tree, &tree.board_sessions(), None, reads, |_| false);
    let sections: Vec<BoardSection> = Section::ALL
        .into_iter()
        .map(|section| BoardSection {
            section: section.into(),
            rows: built
                .rows
                .iter()
                .filter(|r| r.section == section)
                .map(|r| BoardRow {
                    session_id: r.id.0.clone(),
                    kind: match r.kind {
                        board::RowKind::Session => RowKind::Session,
                        board::RowKind::Captain { .. } => RowKind::Captain,
                    },
                    workers: match r.kind {
                        board::RowKind::Session => 0,
                        board::RowKind::Captain { workers } => workers,
                    },
                })
                .collect(),
        })
        .filter(|s| !s.rows.is_empty())
        .collect();
    let answers_of = |s: &Session| {
        answers
            .get(&AgentId(s.id.clone()))
            .filter(|a| a.state == (s.status, s.status_changed_at))
            .and_then(|a| a.answers.clone())
            .unwrap_or_default()
    };
    let now = nebula_core::clock::now_ms();
    let kept = &fetched.focus;
    let off = focus::off(kept, now);
    let found = focus::candidates(&sessions, &sections, &lanes, &off);
    let focus = match focus::shown(kept, &found, fetched.at.is_some(), now) {
        focus::Shown::Orchestrator(set, picks) => Focus {
            source: FocusSource::Orchestrator,
            at: Some(set.at),
            picks: picks
                .into_iter()
                .filter_map(|p| {
                    let c = found.iter().find(|c| c.key == p.key)?;
                    let s = c.sessions.first();
                    Some(FocusPick {
                        key: p.key.clone(),
                        session_id: s.map(|s| s.id.clone()),
                        reason: p.reason.clone(),
                        answers: s.map(|s| answers_of(s)).unwrap_or_default(),
                    })
                })
                .collect(),
            fallback: None,
            rank: None,
        },
        focus::Shown::Rule(fallback) => Focus {
            source: FocusSource::Rule,
            at: kept
                .picks
                .as_ref()
                .filter(|_| fallback.is_some())
                .map(|p| p.at),
            picks: built
                .rows
                .iter()
                .filter(|r| matches!(r.section, Section::NeedsYou | Section::Done))
                .filter_map(|r| sessions.iter().find(|s| s.id == r.id.0))
                .take(focus::PICKS)
                .map(|s| FocusPick {
                    key: s.id.clone(),
                    session_id: Some(s.id.clone()),
                    reason: s.why.clone().unwrap_or_default(),
                    answers: answers_of(s),
                })
                .collect(),
            fallback,
            rank: None,
        },
    };
    let focus = Focus {
        rank: focus::rank(
            kept,
            tree.rank_turn,
            orchestrator.map(|o| (o.status.into(), o.status_changed_at)),
            now,
        ),
        ..focus
    };
    State {
        projects: tree
            .projects
            .iter()
            .filter(|p| !tree.is_orchestrator_home(p))
            .map(|p| Project {
                id: p.id.0.clone(),
                name: p.name.clone(),
                space: spaces.get(&p.repo_path).cloned(),
                team: teams.get(&p.repo_path).cloned(),
            })
            .collect(),
        sessions,
        board: Board {
            sections,
            lanes,
            idle: built.idle,
            stopped: built.stopped,
        },
        focus,
        orchestrator_id: orchestrator.map(|o| o.id.0.clone()),
        tui: tree.tui_open.then(|| TuiState {
            session_id: tree.tui_session.as_ref().map(|id| id.0.clone()),
        }),
        refresh: Refresh {
            running: fetched.running,
            at: fetched.at,
            problems: fetched.problems.clone(),
        },
        today: today.today.clone(),
        today_problem: today.problem.clone(),
        server: Server {
            version: env!("CARGO_PKG_VERSION"),
            protocol: nebula_core::PROTOCOL_VERSION,
        },
    }
}

/// `a` as the contract has a session, with the issue it works on and the
/// pull request on its checkout from `fetched`: `None` when its checkout
/// or project is not in the tree.
fn session(
    tree: &Tree,
    a: &Agent,
    reads: &HashMap<AgentId, BoardRead>,
    fetched: &Fetched,
) -> Option<Session> {
    let worktree = tree.filed_under(&a.worktree_id)?;
    let project = tree.projects.iter().find(|p| p.id == worktree.project_id)?;
    let issue_key = a.issue_key();
    let issue = refresh::joined(&fetched.issues, issue_key.as_deref(), &worktree.branch);
    let pr = fetched
        .prs
        .get(&project.repo_path)
        .and_then(|prs| prs.get(&worktree.path));
    let why = board::read_of(reads, a).and_then(|r| r.line.clone());
    let report_word = match a.status {
        AgentStatus::Finished => why.as_deref().and_then(nebula_core::why::report_word),
        _ => None,
    };
    Some(Session {
        id: a.id.0.clone(),
        name: a.name.clone(),
        project_id: project.id.0.clone(),
        project_name: project.name.clone(),
        branch: worktree.branch.clone(),
        root_checkout: worktree.is_main,
        harness: a.kind.into(),
        role: a.role.into(),
        status: a.status.into(),
        status_changed_at: a.status_changed_at,
        unseen: a.unseen,
        why,
        report_word,
        issue_url: a.issue_url.clone(),
        issue_key,
        issue: issue.map(Issue::from),
        pr_url: pr.map(|pr| pr.url.clone()),
        pr_state: pr.map(|pr| pr.standing().into()),
    })
}

/// The sessions due a BOARD READ in `tree` as it stands ([`board::due_reads`]):
/// the board's rows that say why under them, and every finish read before
/// this client came up, which may have asked and so belong under `needs
/// you`. The ones returned are asked for their screens.
pub fn due_reads(
    tree: &Tree,
    reads: &mut HashMap<AgentId, BoardRead>,
    now_ms: i64,
) -> Vec<AgentId> {
    let order = tree.board_sessions();
    let built = board::Board::build(tree, &order, None, reads, |_| false);
    let mut rows: Vec<AgentId> = built
        .rows
        .into_iter()
        .filter(|r| r.section.reads())
        .map(|r| r.id)
        .collect();
    rows.extend(order.into_iter().filter(|id| {
        tree.agents.iter().any(|a| {
            &a.id == id
                && a.status == AgentStatus::Finished
                && !a.unseen
                && board::read_of(reads, a).is_none()
        })
    }));
    board::due_reads(reads, &tree.agents, &rows, now_ms, board::READ_BEAT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::today::TodayFile;
    use nebula_core::{OutputTail, ProjectId, Worktree, WorktreeId};
    use serde_json::json;

    fn agent(id: &str, worktree: &str, status: AgentStatus, changed_at: i64) -> Agent {
        Agent {
            id: AgentId(id.into()),
            worktree_id: WorktreeId(worktree.into()),
            name: format!("{id} name"),
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

    fn tree() -> Tree {
        let worktree = |id: &str, branch: &str, is_main| Worktree {
            id: WorktreeId(id.into()),
            project_id: ProjectId("p".into()),
            path: PathBuf::from(format!("/code/{id}")),
            branch: branch.into(),
            is_main,
            sort_order: 0,
            removed: false,
        };
        let mut ask = agent("ask", "feat", AgentStatus::NeedsFeedback, 50);
        ask.kind = AgentKind::Codex;
        let mut done = agent("done", "main", AgentStatus::Finished, 40);
        done.unseen = true;
        done.issue_url = Some("https://github.com/o/r/issues/3".into());
        Tree {
            projects: vec![nebula_core::Project {
                id: ProjectId("p".into()),
                name: "orbit".into(),
                repo_path: PathBuf::from("/code/orbit"),
                sort_order: 0,
            }],
            worktrees: vec![
                worktree("main", "main", true),
                worktree("feat", "fix-login", false),
            ],
            agents: vec![
                agent("idle", "main", AgentStatus::Finished, 10),
                agent("run", "main", AgentStatus::Running, 30),
                done,
                ask,
                agent("gone", "feat", AgentStatus::Terminated, 5),
            ],
            ..Tree::default()
        }
    }

    fn landed(reads: &mut HashMap<AgentId, BoardRead>, tree: &Tree, id: &str, screen: &str) {
        let a = tree.agents.iter().find(|a| a.id.0 == id).unwrap();
        reads.insert(
            a.id.clone(),
            BoardRead {
                state: (a.status, a.status_changed_at),
                asked: true,
                line: None,
            },
        );
        let lines: Vec<&str> = screen.lines().collect();
        let tail = OutputTail {
            cols: 60,
            rows: lines.len() as u16,
            end_seq: 1,
            data: lines.join("\r\n").into_bytes(),
        };
        board::land_read(reads, &a.id, Some(&tail));
    }

    /// The STATE as the contract has it: every name as the web app reads
    /// it, the sessions in attention order, the board in sections with the
    /// empty ones left out, and the rule's focus what needs you and what is
    /// done, with their why lines.
    #[test]
    fn the_state_speaks_the_contract() {
        let tree = tree();
        let mut reads = HashMap::new();
        landed(&mut reads, &tree, "done", "⏺ QUESTION: which base?");
        landed(
            &mut reads,
            &tree,
            "ask",
            "  Allow command?\n  › 1. Yes, proceed\n    2. No",
        );
        let spaces = BTreeMap::from([(PathBuf::from("/code/orbit"), "Acme".to_string())]);
        let teams = BTreeMap::from([(PathBuf::from("/code/orbit"), "REL".to_string())]);
        let answers = HashMap::from([
            (
                AgentId("done".into()),
                PickAnswers {
                    state: (SessionStatus::Finished, 40),
                    answers: Some(vec!["main".into(), "dev".into()]),
                },
            ),
            // Read in a state the session has left.
            (
                AgentId("ask".into()),
                PickAnswers {
                    state: (SessionStatus::Running, 49),
                    answers: Some(vec!["stale".into()]),
                },
            ),
        ]);
        let state = serde_json::to_value(state(
            &tree,
            &reads,
            &answers,
            &spaces,
            &teams,
            &Fetched::default(),
            &TodayFile::default(),
        ))
        .unwrap();

        assert_eq!(
            state["projects"],
            json!([{"id": "p", "name": "orbit", "space": "Acme", "team": "REL"}])
        );
        let ids: Vec<&str> = state["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["ask", "run", "done", "idle", "gone"]);
        assert_eq!(
            state["sessions"][0],
            json!({
                "id": "ask",
                "name": "ask name",
                "projectId": "p",
                "projectName": "orbit",
                "branch": "fix-login",
                "rootCheckout": false,
                "harness": "codex",
                "role": "worker",
                "status": "needsFeedback",
                "statusChangedAt": 50,
                "unseen": false,
                "why": "Allow command?",
                "reportWord": null,
                "issueUrl": null,
                "issueKey": null,
                "issue": null,
                "prUrl": null,
                "prState": null,
            })
        );
        assert_eq!(state["sessions"][2]["reportWord"], "QUESTION");
        assert_eq!(
            state["sessions"][2]["issueUrl"],
            "https://github.com/o/r/issues/3"
        );
        assert_eq!(state["sessions"][2]["issueKey"], "#3");
        assert_eq!(
            state["board"],
            json!({
                "sections": [
                    {"section": "needsYou", "rows": [
                        {"sessionId": "ask", "kind": "session", "workers": 0},
                        {"sessionId": "done", "kind": "session", "workers": 0},
                    ]},
                    {"section": "running", "rows": [
                        {"sessionId": "run", "kind": "session", "workers": 0},
                    ]},
                ],
                "lanes": [],
                "idle": 1,
                "stopped": 1,
            })
        );
        assert_eq!(
            state["focus"],
            json!({
                "source": "rule",
                "at": null,
                "picks": [
                    {"key": "ask", "sessionId": "ask", "reason": "Allow command?", "answers": []},
                    {"key": "done", "sessionId": "done", "reason": "QUESTION: which base?", "answers": ["main", "dev"]},
                ],
                "fallback": null,
                "rank": null,
            })
        );
        assert_eq!(state["orchestratorId"], json!(null));
        assert_eq!(state["tui"], json!(null), "no TUI open");
        assert_eq!(
            state["refresh"],
            json!({"running": false, "at": null, "problems": []}),
            "nothing read yet"
        );
        assert_eq!(state["today"], json!(null));
        assert_eq!(state["todayProblem"], json!(null));
        assert_eq!(state["server"]["protocol"], nebula_core::PROTOCOL_VERSION);
    }

    /// The orchestrator's picks are the focus, an issue in a lane among
    /// them, which opens no session, a pick no candidate now left out, and
    /// a rank asked for and held says so; before Linear is read the rule
    /// stands in, saying why.
    #[test]
    fn the_orchestrators_picks_are_the_focus() {
        use crate::focus::{Asked, Kept, Pick, Picks};
        let mut tree = tree();
        tree.rank_turn = nebula_core::RankTurn {
            held_at: Some(1),
            typed_at: None,
        };
        let now = nebula_core::clock::now_ms();
        let pick = |key: &str| Pick {
            key: key.into(),
            reason: format!("{key} first"),
        };
        let mut fetched = Fetched {
            at: Some(now),
            issues: vec![crate::refresh::tests::issue("REL-1", "started", 2, 5)],
            focus: Kept {
                picks: Some(Picks {
                    at: now - 1000,
                    picks: vec![pick("REL-1"), pick("ask"), pick("REL-9")],
                    ranked: Vec::new(),
                }),
                asked: Some(Asked {
                    at: now,
                    auto: false,
                }),
                ..Kept::default()
            },
            ..Fetched::default()
        };
        let state_of = |fetched: &Fetched| {
            serde_json::to_value(state(
                &tree,
                &HashMap::new(),
                &HashMap::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
                fetched,
                &TodayFile::default(),
            ))
            .unwrap()
        };
        let focus = &state_of(&fetched)["focus"];
        assert_eq!(focus["source"], "orchestrator");
        assert_eq!(focus["at"], now - 1000);
        assert_eq!(
            focus["picks"],
            json!([
                {"key": "REL-1", "sessionId": null, "reason": "REL-1 first", "answers": []},
                {"key": "ask", "sessionId": "ask", "reason": "ask first", "answers": []},
            ])
        );
        assert_eq!(focus["fallback"], json!(null));
        assert_eq!(
            focus["rank"],
            json!({"at": now, "auto": false, "state": "held"})
        );
        fetched.at = None;
        let focus = &state_of(&fetched)["focus"];
        assert_eq!(focus["source"], "rule");
        assert_eq!(focus["fallback"], "unread");
        assert_eq!(focus["at"], now - 1000, "the picks the rule stands in for");
        assert_eq!(focus["picks"][0]["key"], "ask");
    }

    /// After a refresh, a session carries the issue it works on and the
    /// pull request on its checkout, the lanes hold the issues no session
    /// works on, and the STATE says when the refresh landed and what it
    /// could not read. Focus stays the sessions'.
    #[test]
    fn a_refresh_joins_issues_and_pull_requests_into_the_state() {
        use crate::refresh::tests::issue;
        use nebula_forge::pull_request::PullRequest;
        let mut tree = tree();
        tree.agents
            .iter_mut()
            .find(|a| a.id.0 == "idle")
            .unwrap()
            .issue_url = Some("https://linear.app/acme/issue/REL-2/root".into());
        let merged = PullRequest {
            number: 9,
            url: "https://github.com/o/r/pull/9".into(),
            title: "Fix login".into(),
            state: "MERGED".into(),
            is_draft: false,
            head: "fix-login".into(),
            head_sha: "abc".into(),
            base: "main".into(),
            health: Default::default(),
            activity: Vec::new(),
        };
        let fetched = Fetched {
            at: Some(77),
            problems: vec!["Linear: Authentication required.".into()],
            issues: vec![
                issue("REL-1", "started", 2, 5),
                issue("REL-2", "unstarted", 0, 5),
                issue("REL-3", "started", 1, 5),
                issue("REL-4", "unstarted", 3, 5),
            ],
            prs: BTreeMap::from([(
                PathBuf::from("/code/orbit"),
                HashMap::from([(PathBuf::from("/code/feat"), merged)]),
            )]),
            ..Fetched::default()
        };
        tree.worktrees
            .iter_mut()
            .find(|w| w.id.0 == "feat")
            .unwrap()
            .branch = "rel-1-fix-login".into();
        let state = state(
            &tree,
            &HashMap::new(),
            &HashMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &fetched,
            &TodayFile::default(),
        );
        let json = serde_json::to_value(&state).unwrap();
        let session = |id: &str| {
            json["sessions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["id"] == id)
                .unwrap()
                .clone()
        };

        let ask = session("ask");
        assert_eq!(ask["issue"]["key"], "REL-1", "by the key in its branch");
        assert_eq!(ask["issue"]["stateType"], "started");
        assert_eq!(ask["issue"]["priority"], 2);
        assert_eq!(ask["prUrl"], "https://github.com/o/r/pull/9");
        assert_eq!(ask["prState"], "merged");
        assert_eq!(session("idle")["issue"]["key"], "REL-2", "by its issue URL");
        assert_eq!(session("run")["issue"], json!(null));
        assert_eq!(
            session("run")["prUrl"],
            json!(null),
            "no PR on the root checkout"
        );
        assert_eq!(
            json["board"]["lanes"],
            json!([
                {"lane": "inFlight", "issues": [Issue::from(&fetched.issues[2])]},
                {"lane": "todo", "issues": [Issue::from(&fetched.issues[3])]},
            ])
        );
        assert_eq!(
            json["board"]["lanes"][0]["issues"][0],
            json!({
                "key": "REL-3",
                "url": "https://linear.app/acme/issue/REL-3",
                "title": "REL-3 title",
                "priority": 1,
                "state": "started",
                "stateType": "started",
                "assignee": "Ada",
                "branchName": "ada/rel-3-work",
                "updatedAt": 5,
                "team": "REL",
                "project": null,
                "started": null,
                "lastProject": null,
            })
        );
        assert_eq!(
            json["refresh"],
            json!({"running": false, "at": 77, "problems": ["Linear: Authentication required."]})
        );
        assert!(state.focus.picks.iter().all(|p| tree
            .agents
            .iter()
            .any(|a| Some(&a.id.0) == p.session_id.as_ref())));
    }

    /// A lane's issue says when Start handed it to which captain, whether
    /// the daemon holds its brief still, why Linear was not moved, and the
    /// project Start last sent its Linear project's issues to.
    #[test]
    fn a_lane_issue_says_what_start_did_with_it() {
        use crate::refresh::tests::issue;
        let mut tree = tree();
        let mut handed = issue("REL-4", "unstarted", 3, 5);
        handed.project = Some("Accounts".into());
        let mut fetched = Fetched {
            issues: vec![handed.clone(), issue("REL-5", "unstarted", 3, 5)],
            ..Fetched::default()
        };
        fetched.starts.handed.insert(
            "REL-4".into(),
            crate::start::Handed {
                project_id: "p".into(),
                project_name: "orbit".into(),
                captain_id: "cap".into(),
                at: 9,
                move_failed: Some("Linear timed out.".into()),
            },
        );
        fetched
            .starts
            .last_project
            .insert("REL/Accounts".into(), "p".into());
        tree.held_briefs = vec![nebula_core::HeldBrief {
            issue_url: handed.url.clone(),
            captain: AgentId("cap".into()),
            at: 9,
        }];
        let issues = |tree: &Tree| {
            let state = state(
                tree,
                &HashMap::new(),
                &HashMap::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
                &fetched,
                &TodayFile::default(),
            );
            serde_json::to_value(state).unwrap()["board"]["lanes"][0]["issues"].clone()
        };
        let lane = issues(&tree);
        assert_eq!(
            lane[0]["started"],
            json!({
                "projectId": "p",
                "projectName": "orbit",
                "captainId": "cap",
                "at": 9,
                "held": true,
                "moveFailed": "Linear timed out.",
            })
        );
        assert_eq!(lane[0]["lastProject"], "p");
        assert_eq!(lane[1]["started"], json!(null));
        assert_eq!(lane[1]["lastProject"], json!(null));
        tree.held_briefs.clear();
        assert_eq!(
            issues(&tree)[0]["started"]["held"],
            false,
            "the captain took it"
        );
    }

    fn picks(tree: &Tree, reads: &HashMap<AgentId, BoardRead>) -> Vec<String> {
        let state = state(
            tree,
            reads,
            &HashMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &Fetched::default(),
            &TodayFile::default(),
        );
        state
            .focus
            .picks
            .into_iter()
            .filter_map(|p| p.session_id)
            .collect()
    }

    fn drop_agents(tree: &mut Tree, ids: &[&str]) {
        tree.agents.retain(|a| !ids.contains(&a.id.0.as_str()));
    }

    /// Puts a second project in `tree` whose captain, `cap`, is at rest: a
    /// row of the board's captains.
    fn add_captain_at_rest(tree: &mut Tree) {
        tree.projects.push(nebula_core::Project {
            id: ProjectId("q".into()),
            name: "ledger".into(),
            repo_path: PathBuf::from("/code/ledger"),
            sort_order: 1,
        });
        tree.worktrees.push(Worktree {
            id: WorktreeId("q-main".into()),
            project_id: ProjectId("q".into()),
            path: PathBuf::from("/code/ledger"),
            branch: "main".into(),
            is_main: true,
            sort_order: 0,
            removed: false,
        });
        let mut captain = agent("cap", "q-main", AgentStatus::Finished, 60);
        captain.role = AgentRole::Captain;
        tree.agents.push(captain);
    }

    /// The rule puts what needs you in focus before what is done, even a
    /// finish that asks behind an unseen one in attention order, and never
    /// a running session or a captain at rest.
    #[test]
    fn focus_by_rule_is_what_needs_you_then_what_is_done() {
        let mut tree = tree();
        drop_agents(&mut tree, &["ask"]);
        add_captain_at_rest(&mut tree);
        let mut reads = HashMap::new();
        landed(&mut reads, &tree, "idle", "⏺ QUESTION: which base?");
        let sections: Vec<SectionName> = state(
            &tree,
            &reads,
            &HashMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &Fetched::default(),
            &TodayFile::default(),
        )
        .board
        .sections
        .iter()
        .map(|s| s.section)
        .collect();
        assert_eq!(
            sections,
            [
                SectionName::NeedsYou,
                SectionName::Done,
                SectionName::Running,
                SectionName::Captains
            ]
        );
        assert_eq!(picks(&tree, &reads), ["idle", "done"]);
    }

    /// A board of running sessions and captains at rest has nothing in
    /// focus, and keeps them in their sections.
    #[test]
    fn focus_by_rule_is_empty_when_nothing_needs_you() {
        let mut tree = tree();
        drop_agents(&mut tree, &["ask", "done"]);
        add_captain_at_rest(&mut tree);
        let state = state(
            &tree,
            &HashMap::new(),
            &HashMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &Fetched::default(),
            &TodayFile::default(),
        );
        assert!(state.focus.picks.is_empty(), "{:?}", state.focus.picks);
        let rows: Vec<(SectionName, &str)> = state
            .board
            .sections
            .iter()
            .flat_map(|s| s.rows.iter().map(|r| (s.section, r.session_id.as_str())))
            .collect();
        assert_eq!(
            rows,
            [
                (SectionName::Running, "run"),
                (SectionName::Captains, "cap")
            ]
        );
    }

    /// The rule puts three in focus at most, the rest left on the board.
    #[test]
    fn focus_by_rule_holds_three_at_most() {
        let mut tree = tree();
        for (id, at) in [("d1", 41), ("d2", 42), ("d3", 43)] {
            let mut done = agent(id, "main", AgentStatus::Finished, at);
            done.unseen = true;
            tree.agents.push(done);
        }
        assert_eq!(picks(&tree, &HashMap::new()), ["ask", "d3", "d2"]);
    }

    /// What the TUIs show, once one is open: the session the one touched
    /// last has up, if any.
    #[test]
    fn the_state_says_what_the_tui_shows() {
        let mut tree = tree();
        let tui = |tree: &Tree| {
            let state = state(
                tree,
                &HashMap::new(),
                &HashMap::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
                &Fetched::default(),
                &TodayFile::default(),
            );
            serde_json::to_value(state).unwrap()["tui"].clone()
        };
        tree.tui_open = true;
        assert_eq!(tui(&tree), json!({"sessionId": null}));
        tree.tui_session = Some(AgentId("run".into()));
        assert_eq!(tui(&tree), json!({"sessionId": "run"}));
    }

    /// A show says whether the TUI had that session up already: only the
    /// session it shows is on screen, and with none there is none.
    #[test]
    fn a_show_says_whether_the_tui_had_the_session_up() {
        let shown = |on: Option<&str>, id: &str| {
            let tui = TuiState {
                session_id: on.map(String::from),
            };
            serde_json::to_value(Shown::of(&tui, id)).unwrap()
        };
        assert_eq!(shown(Some("run"), "run"), json!({"alreadyOnScreen": true}));
        assert_eq!(shown(Some("run"), "ask"), json!({"alreadyOnScreen": false}));
        assert_eq!(shown(None, "run"), json!({"alreadyOnScreen": false}));
    }

    /// A captain stands for its project: its row counts the workers at
    /// work, it is a role of its own, and the orchestrator is a session
    /// but never a row.
    #[test]
    fn a_captain_is_one_row_and_the_orchestrator_none() {
        let mut tree = tree();
        let mut captain = agent("cap", "main", AgentStatus::Finished, 20);
        captain.role = AgentRole::Captain;
        tree.agents.push(captain);
        let mut orchestrator = agent("orch", "main", AgentStatus::Finished, 1);
        orchestrator.role = AgentRole::Orchestrator;
        tree.agents.push(orchestrator);
        let state = serde_json::to_value(state(
            &tree,
            &HashMap::new(),
            &HashMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &Fetched::default(),
            &TodayFile::default(),
        ))
        .unwrap();
        assert_eq!(state["orchestratorId"], "orch");
        assert_eq!(state["projects"][0]["space"], json!(null));
        let roles: Vec<(&str, &str)> = state["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (s["id"].as_str().unwrap(), s["role"].as_str().unwrap()))
            .filter(|(_, role)| *role != "worker")
            .collect();
        assert_eq!(roles, [("cap", "captain"), ("orch", "orchestrator")]);
        let rows: Vec<serde_json::Value> = state["board"]["sections"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s["rows"].as_array().unwrap().clone())
            .collect();
        assert!(rows.iter().all(|r| r["sessionId"] != "orch"), "{rows:?}");
        assert!(
            rows.contains(&json!({"sessionId": "cap", "kind": "captain", "workers": 2})),
            "the asking and the running session are at work: {rows:?}"
        );
    }

    /// Finishes read before this client came up are read once too, since
    /// one may have asked, and the rows that say why are read a beat after
    /// they began.
    #[test]
    fn the_board_reads_its_rows_and_the_finishes_before_it() {
        let tree = tree();
        let mut reads = HashMap::new();
        let mut due = due_reads(&tree, &mut reads, 100_000);
        due.sort_by(|a, b| a.0.cmp(&b.0));
        let due: Vec<&str> = due.iter().map(|id| id.0.as_str()).collect();
        assert_eq!(due, ["ask", "done", "idle"]);
        assert!(
            due_reads(&tree, &mut reads, 100_000).is_empty(),
            "once per state"
        );
    }
}
