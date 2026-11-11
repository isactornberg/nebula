//! The JSON `nebula serve` answers with, mirroring the web app's contract
//! (`app/src/api/contract.ts`) type for type, and the STATE built from the
//! entity tree and the BOARD.

use nebula_client::board::{self, BoardRead, Section};
use nebula_client::Tree;
use nebula_core::{Agent, AgentId, AgentKind, AgentRole, AgentStatus};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

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
    /// Never known here: the daemon keeps a session's pull request to
    /// itself, and the TUI asks `gh` for a checkout's.
    pub pr_url: Option<String>,
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
    pub session_id: String,
    pub reason: String,
    pub answers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Focus {
    pub source: FocusSource,
    pub at: Option<i64>,
    pub picks: Vec<FocusPick>,
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
    pub today: (),
    pub server: Server,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFix {
    pub kind: SkillProblemKind,
    pub title: String,
    pub place: String,
    pub text: String,
    pub fix: String,
    pub skills: Vec<String>,
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
    pub problems: Vec<SkillProblem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skills<'a> {
    pub skills: Vec<&'a Skill>,
    pub fixes: &'a [SkillFix],
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
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    pub error: String,
}

/// How many rows the rule puts in focus at most.
const FOCUS_PICKS: usize = 3;

/// The answers a focus pick's reply offers, read once per state of its
/// session (its status and when that began): `None` while the read is out.
pub struct PickAnswers {
    pub state: (SessionStatus, i64),
    pub answers: Option<Vec<String>>,
}

/// The SPACE each project is filed under, by repo path: the `space` of
/// its `projects` entry in `config.json`, read as the TUI and the daemon
/// read their settings ([`nebula_core::settings`]).
pub fn spaces() -> BTreeMap<PathBuf, String> {
    #[derive(Default, serde::Deserialize)]
    #[serde(default)]
    struct Settings {
        projects: BTreeMap<PathBuf, ProjectSettings>,
    }
    #[derive(Default, serde::Deserialize)]
    #[serde(default)]
    struct ProjectSettings {
        space: String,
    }
    let loaded = nebula_core::settings::load::<Settings>(
        &nebula_core::paths::config_path(),
        &nebula_core::paths::config_local_path(),
    );
    loaded
        .value
        .projects
        .into_iter()
        .filter_map(|(path, p)| {
            let label = p.space.split_whitespace().collect::<Vec<_>>().join(" ");
            (!label.is_empty()).then_some((path, label))
        })
        .collect()
}

/// The STATE of `tree`, with `reads` what the board read off each
/// session's screen, `answers` what the focus picks' replies offer, and
/// `spaces` the SPACE of each project by repo path. The rule's focus is
/// the board's rows that need you, then the ones done, in attention order:
/// a running session or a captain at rest is never one.
pub fn state(
    tree: &Tree,
    reads: &HashMap<AgentId, BoardRead>,
    answers: &HashMap<AgentId, PickAnswers>,
    spaces: &BTreeMap<PathBuf, String>,
) -> State {
    let orchestrator = tree.orchestrator_agent();
    let mut order = tree.attention_sessions();
    if let Some(o) = orchestrator.filter(|o| !order.contains(&o.id)) {
        order.push(o.id.clone());
    }
    let sessions: Vec<Session> = order
        .iter()
        .filter_map(|id| tree.agents.iter().find(|a| &a.id == id))
        .filter_map(|a| session(tree, a, reads))
        .collect();
    let built = board::Board::build(tree, &tree.board_sessions(), None, reads, |_| false);
    let sections = Section::ALL
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
    let picks = built
        .rows
        .iter()
        .filter(|r| matches!(r.section, Section::NeedsYou | Section::Done))
        .filter_map(|r| sessions.iter().find(|s| s.id == r.id.0))
        .take(FOCUS_PICKS)
        .map(|s| FocusPick {
            session_id: s.id.clone(),
            reason: s.why.clone().unwrap_or_default(),
            answers: answers
                .get(&AgentId(s.id.clone()))
                .filter(|a| a.state == (s.status, s.status_changed_at))
                .and_then(|a| a.answers.clone())
                .unwrap_or_default(),
        })
        .collect();
    State {
        projects: tree
            .projects
            .iter()
            .filter(|p| !tree.is_orchestrator_home(p))
            .map(|p| Project {
                id: p.id.0.clone(),
                name: p.name.clone(),
                space: spaces.get(&p.repo_path).cloned(),
            })
            .collect(),
        sessions,
        board: Board {
            sections,
            idle: built.idle,
            stopped: built.stopped,
        },
        focus: Focus {
            source: FocusSource::Rule,
            at: None,
            picks,
        },
        orchestrator_id: orchestrator.map(|o| o.id.0.clone()),
        tui: tree.tui_open.then(|| TuiState {
            session_id: tree.tui_session.as_ref().map(|id| id.0.clone()),
        }),
        today: (),
        server: Server {
            version: env!("CARGO_PKG_VERSION"),
            protocol: nebula_core::PROTOCOL_VERSION,
        },
    }
}

/// `a` as the contract has a session: `None` when its checkout or project
/// is not in the tree.
fn session(tree: &Tree, a: &Agent, reads: &HashMap<AgentId, BoardRead>) -> Option<Session> {
    let worktree = tree.filed_under(&a.worktree_id)?;
    let project = tree.projects.iter().find(|p| p.id == worktree.project_id)?;
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
        issue_key: a.issue_key(),
        pr_url: None,
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
        let state = serde_json::to_value(state(&tree, &reads, &answers, &spaces)).unwrap();

        assert_eq!(
            state["projects"],
            json!([{"id": "p", "name": "orbit", "space": "Acme"}])
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
                "prUrl": null,
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
                    {"sessionId": "ask", "reason": "Allow command?", "answers": []},
                    {"sessionId": "done", "reason": "QUESTION: which base?", "answers": ["main", "dev"]},
                ],
            })
        );
        assert_eq!(state["orchestratorId"], json!(null));
        assert_eq!(state["tui"], json!(null), "no TUI open");
        assert_eq!(state["today"], json!(null));
        assert_eq!(state["server"]["protocol"], nebula_core::PROTOCOL_VERSION);
    }

    fn picks(tree: &Tree, reads: &HashMap<AgentId, BoardRead>) -> Vec<String> {
        let state = state(tree, reads, &HashMap::new(), &BTreeMap::new());
        state
            .focus
            .picks
            .into_iter()
            .map(|p| p.session_id)
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
        let sections: Vec<SectionName> = state(&tree, &reads, &HashMap::new(), &BTreeMap::new())
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
        let state = state(&tree, &HashMap::new(), &HashMap::new(), &BTreeMap::new());
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
            let state = state(tree, &HashMap::new(), &HashMap::new(), &BTreeMap::new());
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
