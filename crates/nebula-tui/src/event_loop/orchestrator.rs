//! The ORCHESTRATOR VIEW (`F10`): the orchestrator session full-screen,
//! with every live session in a strip across its header in attention
//! order. A session from the strip, from `F9`, or from the orchestrator's
//! own `nebula orchestrator review` takes the screen the same way, and the
//! way back from it is the orchestrator. It is a full-screen session
//! (`launcher::toggle_full_screen`), so the keys, the hatch and the drawing
//! are the ones a full-screen session already has, but it never moves the
//! grid's cursor: leaving it finds the grid as it was.
//!
//! The orchestrator lives in the ORCHESTRATOR HOME, nebula's own folder for
//! it (`app::orchestrator_home`). The first `F10` makes that folder a
//! project, which no list of projects shows, and starts the orchestrator in
//! its ROOT WORKTREE.

use super::{attach_now, leave_terminal_lock, zoom_pane};
use crate::app::{App, PendingIntent};
use nebula_core::{AgentId, AgentKind, AgentRole, ClientRequest, SessionRef};

/// What the KEY COMBO DISPLAY says each move did.
pub(super) const OPENED: &str = "Orchestrator";
pub(super) const BACK_TO_ORCHESTRATOR: &str = "Back to the orchestrator";
pub(super) const BACK_TO_GRID: &str = "Back to the grid";
const STARTING: &str = "starting the orchestrator…";

/// `F10`: the view, or out of it from anywhere in it.
pub(super) fn toggle(app: &mut App, out: &mut Vec<ClientRequest>) -> &'static str {
    if app.orchestrator_view() {
        leave(app, out);
        return BACK_TO_GRID;
    }
    open(app, out);
    OPENED
}

/// The view up on the orchestrator, started first when there is none:
/// `F10` from the grid, and a launch with START IN THE ORCHESTRATOR on.
pub(super) fn open(app: &mut App, out: &mut Vec<ClientRequest>) {
    match app.orchestrator_agent().map(|a| a.id.clone()) {
        Some(id) => show_session(app, SessionRef::Agent(id), out),
        None => start(app, out),
    }
}

/// A session from the strip, `F9`, or the orchestrator's `review`: it takes
/// the screen, and the view comes up around it if it was not. The grid's
/// cursor stays where it is.
pub(crate) fn show_session(app: &mut App, sref: SessionRef, out: &mut Vec<ClientRequest>) {
    // A Claude Cloud row runs in the browser, with no PTY to show.
    if let Some(url) = super::cloud_session_url_of(app, &sref) {
        super::open_link(app, &url, out);
        return;
    }
    // The guard in `attach_inner` keeps everything but the view itself off
    // the pane while the view is up, so it stands down for the swap.
    app.launcher.orchestrator = false;
    app.launcher.board_focus = None;
    attach_now(app, sref, out);
    app.launcher.orchestrator = zoom_pane(app, out);
}

/// The view's way back (`^q`, `^F`): from any other session to
/// the orchestrator, and from the orchestrator out to the grid.
pub(super) fn back(app: &mut App, out: &mut Vec<ClientRequest>) -> &'static str {
    let orchestrator = app
        .orchestrator_agent()
        .map(|a| SessionRef::Agent(a.id.clone()));
    let showing = app.pane.term.as_ref().map(|t| t.sref.clone());
    match orchestrator {
        Some(sref) if showing.as_ref() != Some(&sref) => {
            show_session(app, sref, out);
            BACK_TO_ORCHESTRATOR
        }
        _ => {
            leave(app, out);
            BACK_TO_GRID
        }
    }
}

/// Out to the grid (`F10`, the crumb): the pane goes back to the card
/// under the grid's cursor, which the view never moved. With no card there
/// to go back to (a project with no sessions, the aim let go), the pane is
/// emptied, rather than left on what the view last showed.
pub(super) fn leave(app: &mut App, out: &mut Vec<ClientRequest>) {
    app.launcher.board_focus = None;
    leave_terminal_lock(app);
    let card = (!app.launcher.launcher_unaimed && !app.on_folded_band())
        .then(|| app.selected_session_row().and_then(|row| row.sref()))
        .flatten();
    if card.is_some() {
        super::preview_selected_now(app, out);
    }
    if app
        .pane
        .term
        .as_ref()
        .is_some_and(|t| Some(&t.sref) != card.as_ref())
    {
        super::detach_pane(app, out);
    }
}

pub(super) const BOARD_FOCUSED: &str = "Keys to the board";
pub(super) const BOARD_UNFOCUSED: &str = "Keys back to the session";
const NO_BOARD: &str = "the board needs a window 100 columns wide";
const EMPTY_BOARD: &str = "no session on the board to move to";

/// `F8` in the view: the keys go to the BOARD, its highlight on the row
/// of the session on screen, or its first row; pressed again they go back
/// to the session. Nothing to take them in a screen too narrow for the
/// board, or with no row on it.
pub(super) fn focus_board(app: &mut App) -> &'static str {
    app.chrome.dirty = true;
    if app.launcher.board_focus.take().is_some() {
        return BOARD_UNFOCUSED;
    }
    if !app.launcher.board_shown {
        app.chrome.flash = Some(NO_BOARD.into());
        return NO_BOARD;
    }
    let rows = crate::board::board(app).rows;
    let showing = app.pane.term.as_ref().map(|t| t.sref.clone());
    let at = rows
        .iter()
        .find(|r| showing == Some(SessionRef::Agent(r.id.clone())))
        .or(rows.first());
    let Some(row) = at else {
        app.chrome.flash = Some(EMPTY_BOARD.into());
        return EMPTY_BOARD;
    };
    app.launcher.board_focus = Some(row.id.clone());
    app.launcher.board_scroll = None;
    BOARD_FOCUSED
}

/// A key while the BOARD has them: `j`/`k` or the arrows move the
/// highlight a row, stopping at the ends, Enter shows its session and
/// hands it the keys, and Esc hands them back to the session on screen.
/// Every other key is dropped, so nothing typed here reaches a session.
pub(super) fn board_key(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    out: &mut Vec<ClientRequest>,
) {
    use crossterm::event::KeyCode;
    app.chrome.dirty = true;
    let step: i64 = match key.code {
        KeyCode::Esc => {
            app.launcher.board_focus = None;
            return;
        }
        KeyCode::Enter => {
            if let Some(id) = app.launcher.board_focus.take() {
                if app.tree.agents.iter().any(|a| a.id == id) {
                    show_session(app, SessionRef::Agent(id), out);
                }
            }
            return;
        }
        KeyCode::Char('j') | KeyCode::Down => 1,
        KeyCode::Char('k') | KeyCode::Up => -1,
        _ => return,
    };
    let rows = crate::board::board(app).rows;
    if rows.is_empty() {
        app.launcher.board_focus = None;
        return;
    }
    // A row that left the board under the highlight: start over at the top.
    let at = app
        .launcher
        .board_focus
        .as_ref()
        .and_then(|id| rows.iter().position(|r| r.id == *id));
    let next = match at {
        Some(at) => (at as i64 + step).clamp(0, rows.len() as i64 - 1) as usize,
        None => 0,
    };
    app.launcher.board_focus = Some(rows[next].id.clone());
    app.launcher.board_scroll = None;
}

/// What `F9` and `⇧F9` say they did.
pub(super) const NEXT: &str = "Next session needing you";
pub(super) const PREVIOUS: &str = "Previous session needing you";

/// `F9` (`step` 1) and `⇧F9` (-1), from anywhere: the next row of the
/// walk ([`App::walk_ring`]) after the session on screen, shown in the
/// view. A session `F9` showed that has left the walk since, answered or
/// read, is passed by from the place it had, so the rest of the queue is
/// not skipped. From the orchestrator, or from outside the view, the walk
/// starts at the head, or the tail going back, as `.` and `,` do.
pub(super) fn walk(app: &mut App, step: i64, out: &mut Vec<ClientRequest>) -> &'static str {
    let did = if step > 0 { NEXT } else { PREVIOUS };
    let ring = app.walk_ring();
    let showing = app
        .orchestrator_view()
        .then(|| app.pane.term.as_ref().map(|t| t.sref.clone()))
        .flatten();
    // Nothing to walk to, or only the session already on screen: the
    // screen stays as it is, and says so.
    let only = |id: &AgentId| showing == Some(SessionRef::Agent(id.clone()));
    if ring.is_empty() || (ring.len() == 1 && only(&ring[0])) {
        app.chrome.flash = Some(NOTHING_TO_WALK.into());
        return did;
    }
    // The place to step from: where the session on screen is in the walk,
    // or, gone from it, just before the place it had (going on) or that
    // place itself (going back), the rows after it having moved up one.
    let from = match showing {
        Some(SessionRef::Agent(id)) => match ring.iter().position(|r| *r == id) {
            Some(i) => Some(i as i64),
            None => app
                .launcher
                .walked
                .as_ref()
                .filter(|(walked, _)| *walked == id)
                .map(|(_, at)| *at as i64 - i64::from(step > 0)),
        },
        _ => None,
    };
    let len = ring.len() as i64;
    let next = match from {
        Some(i) => (i + step).rem_euclid(len) as usize,
        None if step > 0 => 0,
        None => ring.len() - 1,
    };
    app.launcher.walked = Some((ring[next].clone(), next));
    app.launcher.board_scroll = None;
    show_session(app, SessionRef::Agent(ring[next].clone()), out);
    did
}

/// What `F9` flashes with nothing to bring up.
const NOTHING_TO_WALK: &str = "no other session to show";

/// `F10` with no orchestrator: start one in the ORCHESTRATOR HOME, making
/// the folder a project first when it is not one yet.
fn start(app: &mut App, out: &mut Vec<ClientRequest>) {
    let starting = app.requests.orchestrator_when_home
        || app.requests.pending.values().any(|i| {
            matches!(
                i,
                PendingIntent::AddOrchestratorHome | PendingIntent::OpenCreatedOrchestrator
            )
        });
    if starting {
        app.chrome.flash = Some(STARTING.into());
        return;
    }
    if let Some(worktree) = app.tree.orchestrator_home_root().map(|w| w.id.clone()) {
        create(app, worktree, out);
        return;
    }
    let req_id = app.alloc_req_id(PendingIntent::AddOrchestratorHome);
    out.push(ClientRequest::AddProject {
        req_id,
        path: crate::app::orchestrator_home().to_path_buf(),
        name: Some("orchestrator".into()),
        create_missing: true,
    });
    app.chrome.flash = Some(STARTING.into());
}

/// The Ack of the home's AddProject: start the orchestrator once its ROOT
/// WORKTREE is in the tree, unless the user moved on meanwhile.
pub(super) fn home_added(app: &mut App, follow: bool, out: &mut Vec<ClientRequest>) {
    if follow {
        app.requests.orchestrator_when_home = true;
        home_landed(app, out);
    }
}

/// After every upsert, and the home's Ack: the ROOT WORKTREE's upsert and
/// the Ack land in either order, and the later one starts the orchestrator.
pub(super) fn home_landed(app: &mut App, out: &mut Vec<ClientRequest>) {
    if !app.requests.orchestrator_when_home {
        return;
    }
    if let Some(worktree) = app.tree.orchestrator_home_root().map(|w| w.id.clone()) {
        app.requests.orchestrator_when_home = false;
        create(app, worktree, out);
    }
}

fn create(app: &mut App, worktree: nebula_core::WorktreeId, out: &mut Vec<ClientRequest>) {
    let req_id = app.alloc_req_id(PendingIntent::OpenCreatedOrchestrator);
    out.push(ClientRequest::CreateAgent {
        req_id,
        worktree,
        name: "orchestrator".into(),
        kind: AgentKind::Claude,
        custom_harness: None,
        model: None,
        effort: None,
        auto_title: false,
        cloud_prompt: None,
        starting_prompt: None,
        issue_url: None,
        role: AgentRole::Orchestrator,
    });
    app.chrome.flash = Some(STARTING.into());
}

/// The Ack of the orchestrator's create: the view comes up on it, unless
/// the user moved on meanwhile. Either way it has started, and the flash
/// saying it was starting goes.
pub(super) fn created(app: &mut App, id: AgentId, follow: bool, out: &mut Vec<ClientRequest>) {
    if app.chrome.flash.as_deref() == Some(STARTING) {
        app.chrome.flash = None;
    }
    if follow {
        show_session(app, SessionRef::Agent(id), out);
    }
}

/// What the footer says when the orchestrator on screen asks something
/// before it has had a prompt: Claude's question whether to trust its
/// folder, on its first start, whose default answer quits.
pub(super) const FIRST_QUESTION: &str = "to start the orchestrator, pick Yes, I trust this folder";

/// `agent` on screen went to needs you: when it is the orchestrator, in the
/// view, and asking before it has had a prompt, the footer says what it
/// asks and how it is answered.
pub(super) fn explain_first_question(app: &mut App, agent: &AgentId) {
    let first = app
        .orchestrator_agent()
        .is_some_and(|a| a.id == *agent && a.recent_prompts.is_empty());
    if first && app.orchestrator_view() {
        app.chrome.flash = Some(FIRST_QUESTION.into());
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{
        buffer_text, hse, press, seed_tree, with_config_json, with_default_config,
    };
    use super::super::{
        handle_server_event, handle_terminal_event, jump_to_target, ui_state_json, Landing,
    };
    use crate::app::{App, HitTarget};
    use crate::palette::PaletteTarget;
    use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use nebula_core::{
        Agent, AgentId, AgentKind, AgentRole, AgentStatus, ClientRequest, Entity, EntityId,
        Project, ProjectId, ServerEvent, SessionRef, Worktree, WorktreeId,
    };
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn agent_row(
        id: &str,
        worktree: &str,
        name: &str,
        status: AgentStatus,
        role: AgentRole,
    ) -> Agent {
        Agent {
            id: AgentId(id.into()),
            worktree_id: WorktreeId(worktree.into()),
            name: name.into(),
            status,
            archived: false,
            archived_at: 0,
            unseen: false,
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            sort_order: 1,
            status_changed_at: crate::app::now_ms(),
            alive: true,
            issue_url: None,
            role,
            recent_prompts: Vec::new(),
        }
    }

    fn upsert(app: &mut App, entity: Entity) -> Vec<ClientRequest> {
        server(app, ServerEvent::EntityUpserted { entity })
    }

    fn server(app: &mut App, event: ServerEvent) -> Vec<ClientRequest> {
        let mut out = Vec::new();
        handle_server_event(app, event, &mut out);
        out
    }

    /// The ORCHESTRATOR HOME as the daemon keeps it: a project at the home
    /// folder, with its ROOT WORKTREE `wh`.
    fn seed_home(app: &mut App) -> Vec<ClientRequest> {
        upsert(
            app,
            Entity::Project(Project {
                id: ProjectId("ph".into()),
                name: "orchestrator".into(),
                repo_path: crate::app::orchestrator_home().to_path_buf(),
                sort_order: 5,
            }),
        );
        upsert(
            app,
            Entity::Worktree(Worktree {
                id: WorktreeId("wh".into()),
                project_id: ProjectId("ph".into()),
                path: crate::app::orchestrator_home().to_path_buf(),
                branch: "main".into(),
                is_main: true,
                sort_order: 0,
                removed: false,
            }),
        )
    }

    /// `seed_tree`'s `demo` with `agent-1` under the grid's cursor, and the
    /// orchestrator in its home when `with_orchestrator`.
    fn app(with_orchestrator: bool) -> App {
        let mut app = App::new();
        seed_tree(&mut app);
        if with_orchestrator {
            seed_home(&mut app);
            upsert(
                &mut app,
                Entity::Agent(agent_row(
                    "o1",
                    "wh",
                    "orchestrator",
                    AgentStatus::Running,
                    AgentRole::Orchestrator,
                )),
            );
        }
        jump_to_target(
            &mut app,
            PaletteTarget::Session(AgentId("a1".into())),
            Landing::FocusOnly,
            &mut Vec::new(),
        );
        draw(&mut app);
        app
    }

    /// A second worker beside `agent-1`, stopped on a question: the head
    /// of the attention order.
    fn seed_asking(app: &mut App) {
        upsert(
            app,
            Entity::Agent(agent_row(
                "a2",
                "w1",
                "asking",
                AgentStatus::NeedsFeedback,
                AgentRole::Worker,
            )),
        );
    }

    /// A frame too narrow for the BOARD: the header's strip stands in.
    fn draw(app: &mut App) -> String {
        draw_at(app, 90, 30)
    }

    /// A frame with room for the BOARD.
    fn draw_wide(app: &mut App) -> String {
        draw_at(app, 120, 30)
    }

    fn draw_at(app: &mut App, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| crate::ui::draw(f, app)).unwrap();
        buffer_text(&terminal)
    }

    /// Beside `agent-1` (idle): a session asking, an unread finish and a
    /// running one, the last two in a project of their own.
    fn seed_board(app: &mut App) {
        seed_asking(app);
        upsert(
            app,
            Entity::Project(Project {
                id: ProjectId("p2".into()),
                name: "api-server".into(),
                repo_path: "/tmp/api-server".into(),
                sort_order: 1,
            }),
        );
        upsert(
            app,
            Entity::Worktree(Worktree {
                id: WorktreeId("w2".into()),
                project_id: ProjectId("p2".into()),
                path: "/tmp/api-server".into(),
                branch: "main".into(),
                is_main: true,
                sort_order: 0,
                removed: false,
            }),
        );
        let mut finished = agent_row(
            "a3",
            "w2",
            "JSON flag for lists",
            AgentStatus::Finished,
            AgentRole::Worker,
        );
        finished.unseen = true;
        upsert(app, Entity::Agent(finished));
        upsert(
            app,
            Entity::Agent(agent_row(
                "a4",
                "w2",
                "Token store on sqlite",
                AgentStatus::Running,
                AgentRole::Worker,
            )),
        );
    }

    /// The text of the board's column, its seam left off, in a frame
    /// `text` of a 120-wide screen: a line per row, trailing blanks
    /// trimmed.
    fn board_column(text: &str) -> Vec<String> {
        let w = usize::from(crate::board::width(120)) - 1;
        text.lines()
            .skip(3)
            .map(|l| l.chars().take(w).collect::<String>().trim_end().to_string())
            .collect()
    }

    fn key(app: &mut App, code: KeyCode, mods: KeyModifiers) -> Vec<ClientRequest> {
        let mut out = Vec::new();
        press(app, code, mods, &mut out);
        out
    }

    fn f10(app: &mut App) -> Vec<ClientRequest> {
        key(app, KeyCode::F(10), KeyModifiers::NONE)
    }

    fn f9(app: &mut App, mods: KeyModifiers) -> Vec<ClientRequest> {
        key(app, KeyCode::F(9), mods)
    }

    fn hatch(app: &mut App) {
        key(app, KeyCode::Char('q'), KeyModifiers::CONTROL);
    }

    fn click(app: &mut App, hit: HitTarget) -> Vec<ClientRequest> {
        let mut out = Vec::new();
        let (rect, _) = app
            .chrome
            .hits
            .iter()
            .find(|(_, h)| *h == hit)
            .cloned()
            .unwrap_or_else(|| panic!("{hit:?} was not drawn"));
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            let event = MouseEvent {
                kind,
                column: rect.x,
                row: rect.y,
                modifiers: KeyModifiers::NONE,
            };
            handle_terminal_event(app, crossterm::event::Event::Mouse(event), &mut out);
        }
        out
    }

    fn showing(app: &App) -> Option<SessionRef> {
        app.pane.term.as_ref().map(|t| t.sref.clone())
    }

    fn cursor(app: &App) -> Option<SessionRef> {
        app.selected_session_row().and_then(|r| r.sref())
    }

    fn agent(id: &str) -> SessionRef {
        SessionRef::Agent(AgentId(id.into()))
    }

    /// A relaunch: a fresh client handed `app`'s tree and saved UI state,
    /// as the first Snapshot does.
    fn relaunched(app: &App) -> (App, Vec<ClientRequest>) {
        let tree = app.tree.clone();
        let mut next = App::new();
        let out = server(
            &mut next,
            ServerEvent::Snapshot {
                projects: tree.projects,
                worktrees: tree.worktrees,
                agents: tree.agents,
                terminals: tree.terminals,
                links: tree.links,
                pr_seen: Vec::new(),
                ui_state: Some(ui_state_json(app)),
            },
        );
        (next, out)
    }

    #[test]
    fn the_board_lists_what_needs_you_by_section() {
        with_default_config(|| {
            let mut app = app(true);
            seed_board(&mut app);
            draw_wide(&mut app);
            f10(&mut app);
            let text = draw_wide(&mut app);
            assert!(text.contains("‹ sessions / ● orchestrator "), "{text}");
            assert!(!text.contains("● agent-1"), "no strip: {text}");
            let column = board_column(&text);
            assert_eq!(
                column[..14],
                [
                    "  1 needs you · 1 done · 1 running",
                    "",
                    "  needs you",
                    "  ● demo  asking                   now",
                    "",
                    "",
                    "  done",
                    "  ● api-server  JSON flag for li…  now",
                    "",
                    "",
                    "  running",
                    "  ● api-s…  Token store on sqlite  now",
                    "",
                    "  1 idle",
                ],
                "a `needs you` and a `done` row keep a line for why, blank until read: {text}"
            );
            let seam = usize::from(crate::board::width(120)) - 1;
            for line in text.lines().skip(3).take(25) {
                assert_eq!(
                    line.chars().nth(seam),
                    Some('│'),
                    "nothing past the seam: {text}"
                );
            }
        });
    }

    /// A project with a captain is the captain's row, in the section its
    /// state puts it, counting the workers it has at work; its sessions
    /// that need you are rows of their own, the rest fold into it, and
    /// one put on screen comes back as its own lit row.
    #[test]
    fn a_projects_captain_stands_for_its_sessions_on_the_board() {
        with_default_config(|| {
            let mut app = app(true);
            seed_board(&mut app);
            upsert(
                &mut app,
                Entity::Agent(agent_row(
                    "c1",
                    "w2",
                    "captain",
                    AgentStatus::Running,
                    AgentRole::Captain,
                )),
            );
            upsert(
                &mut app,
                Entity::Agent(agent_row(
                    "a5",
                    "w2",
                    "Migrate tokens",
                    AgentStatus::NeedsFeedback,
                    AgentRole::Worker,
                )),
            );
            draw_wide(&mut app);
            f10(&mut app);
            let column = board_column(&draw_wide(&mut app));
            assert_eq!(
                column[..15],
                [
                    "  2 need you · 1 running",
                    "",
                    "  needs you",
                    "  ● demo  asking                   now",
                    "",
                    "  ● api-server  Migrate tokens     now",
                    "",
                    "",
                    "  running",
                    "  ● api-serv… captain · 2 workers  now",
                    "",
                    "  1 idle",
                    "",
                    "",
                    "",
                ],
                "the unread finish and the running worker fold into it: {column:#?}"
            );

            // At rest, it has a section of its own the header leaves out.
            moved(&mut app, "c1", AgentStatus::Finished);
            let column = board_column(&draw_wide(&mut app));
            assert_eq!(column[0], "  2 need you");
            let at = column.iter().position(|l| l == "  captains").unwrap();
            assert_eq!(column[at + 1], "  ● api-serv… captain · 2 workers  now");
            // The project is whole in a board with room for it.
            let wide = draw_at(&mut app, 160, 30);
            assert!(
                wide.contains("● api-server captain · 2 workers              now"),
                "{wide}"
            );

            // Its report waits under `done`, and F9 walks to it.
            let mut done = app
                .tree
                .agents
                .iter()
                .find(|a| a.id.0 == "c1")
                .cloned()
                .unwrap();
            done.unseen = true;
            upsert(&mut app, Entity::Agent(done));
            let column = board_column(&draw_wide(&mut app));
            assert_eq!(column[0], "  2 need you · 1 done");
            let at = column.iter().position(|l| l == "  done").unwrap();
            assert_eq!(column[at + 1], "  ● api-serv… captain · 2 workers  now");
            assert!(app.walk_ring().contains(&AgentId("c1".into())));

            // A worker it stands for, put on screen, is a row again.
            click(&mut app, HitTarget::OrchestratorChip(agent("c1")));
            server(
                &mut app,
                ServerEvent::ReviewOpened {
                    opener: AgentId("o1".into()),
                    sessions: vec![agent("a4")],
                    tabs: Vec::new(),
                },
            );
            assert_eq!(showing(&app), Some(agent("a4")));
            let column = board_column(&draw_wide(&mut app));
            assert!(
                column.iter().any(|l| l.contains("Token store on sqlite")),
                "{column:#?}"
            );
        });
    }

    /// A captain at rest put on screen stays under `captains`, lit there:
    /// reading a captain is not a finish of its own.
    #[test]
    fn a_captain_at_rest_put_on_screen_stays_under_captains() {
        with_default_config(|| {
            let mut app = app(true);
            seed_board(&mut app);
            for (id, worktree) in [("c1", "w2"), ("c2", "w1")] {
                upsert(
                    &mut app,
                    Entity::Agent(agent_row(
                        id,
                        worktree,
                        "captain",
                        AgentStatus::Finished,
                        AgentRole::Captain,
                    )),
                );
            }
            draw_wide(&mut app);
            f10(&mut app);
            draw_wide(&mut app);
            click(&mut app, HitTarget::OrchestratorChip(agent("c1")));
            assert_eq!(showing(&app), Some(agent("c1")));
            let column = board_column(&draw_wide(&mut app));
            assert_eq!(column[0], "  1 needs you", "{column:#?}");
            assert!(!column.iter().any(|l| l == "  done"), "{column:#?}");
            let at = column.iter().position(|l| l == "  captains").unwrap();
            let mut captains = column[at + 1..at + 3].to_vec();
            captains.sort();
            assert_eq!(
                captains,
                [
                    "  ● api-server captain · 1 worker  now",
                    "  ● demo captain · 1 worker        now",
                ]
            );

            // The answers its reply ends on are its chat's, never the board's.
            land_read(
                &mut app,
                "c1",
                &["⏺ DONE checked the docs", "  NEXT: Fix all | Leave it"],
            );
            let column = board_column(&draw_wide(&mut app));
            assert!(!column.iter().any(|l| l.contains("Fix all")), "{column:#?}");
        });
    }

    /// `agent_row` with `status` changed, as the daemon's next upsert of
    /// it says.
    fn moved(app: &mut App, id: &str, status: AgentStatus) {
        let mut row = app
            .tree
            .agents
            .iter()
            .find(|a| a.id.0 == id)
            .cloned()
            .unwrap();
        row.status = status;
        row.unseen = false;
        upsert(app, Entity::Agent(row));
    }

    #[test]
    fn f9_walks_what_waits_on_you_and_a_shown_finish_keeps_its_row() {
        with_default_config(|| {
            let mut app = app(true);
            seed_board(&mut app);
            draw_wide(&mut app);
            f10(&mut app);
            let mut sent = f9(&mut app, KeyModifiers::NONE);
            assert_eq!(showing(&app), Some(agent("a2")), "needs you first");
            sent.extend(f9(&mut app, KeyModifiers::NONE));
            assert_eq!(showing(&app), Some(agent("a3")), "then done");
            // Putting it up read it: the daemon says so.
            moved(&mut app, "a3", AgentStatus::Finished);
            let text = draw_wide(&mut app);
            assert!(
                text.contains("‹ sessions / orchestrator / ● JSON flag for lists"),
                "{text}"
            );
            assert!(
                board_column(&text).iter().any(|l| l.contains("JSON flag")),
                "the one on screen keeps its row: {text}"
            );
            sent.extend(f9(&mut app, KeyModifiers::NONE));
            assert_eq!(
                showing(&app),
                Some(agent("a2")),
                "while one still needs you, the running one waits"
            );
            let text = draw_wide(&mut app);
            assert!(
                !board_column(&text).iter().any(|l| l.contains("JSON flag")),
                "read and gone from the screen, it leaves the board: {text}"
            );

            // Answered, nothing waits: the running rows join the walk.
            moved(&mut app, "a2", AgentStatus::Running);
            sent.extend(f9(&mut app, KeyModifiers::NONE));
            assert_eq!(showing(&app), Some(agent("a4")));
            sent.extend(f9(&mut app, KeyModifiers::NONE));
            assert_eq!(showing(&app), Some(agent("a2")), "round to the head");
            sent.extend(f9(&mut app, KeyModifiers::SHIFT));
            assert_eq!(showing(&app), Some(agent("a4")), "⇧F9 walks back");
            assert!(
                !sent
                    .iter()
                    .any(|r| matches!(r, ClientRequest::Input { .. })),
                "F9 is nebula's, never the agent's: {sent:?}"
            );
        });
    }

    #[test]
    fn f9_after_an_answer_goes_on_down_the_queue() {
        with_default_config(|| {
            let mut app = app(true);
            seed_board(&mut app);
            for (id, name) in [("a5", "second ask"), ("a6", "third ask")] {
                upsert(
                    &mut app,
                    Entity::Agent(agent_row(
                        id,
                        "w2",
                        name,
                        AgentStatus::NeedsFeedback,
                        AgentRole::Worker,
                    )),
                );
            }
            draw_wide(&mut app);
            f10(&mut app);
            let queue = app.walk_ring();
            assert_eq!(queue.len(), 4, "three asking and the finish: {queue:?}");
            let shown = |app: &App| match showing(app) {
                Some(SessionRef::Agent(id)) => id,
                other => panic!("{other:?}"),
            };
            f9(&mut app, KeyModifiers::NONE);
            f9(&mut app, KeyModifiers::NONE);
            assert_eq!(shown(&app), queue[1]);
            moved(&mut app, &queue[1].0, AgentStatus::Running);
            f9(&mut app, KeyModifiers::NONE);
            assert_eq!(
                shown(&app),
                queue[2],
                "the next one asking, not the row after where the answered one went"
            );
            moved(&mut app, &queue[2].0, AgentStatus::Running);
            f9(&mut app, KeyModifiers::SHIFT);
            assert_eq!(shown(&app), queue[0], "⇧F9 goes back from the place it had");
        });
    }

    #[test]
    fn a_lit_two_line_row_has_its_accent_bar_down_both_lines() {
        with_default_config(|| {
            let mut app = app(true);
            seed_board(&mut app);
            draw_wide(&mut app);
            f10(&mut app);
            f9(&mut app, KeyModifiers::NONE);
            let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
            terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
            let (row, _) = app
                .chrome
                .hits
                .iter()
                .find(|(_, h)| *h == HitTarget::OrchestratorChip(agent("a2")))
                .cloned()
                .unwrap();
            assert_eq!(row.height, 2, "the title and the line under it");
            let accent = app.chrome.theme.accent;
            for y in row.y..row.bottom() {
                let bar = &terminal.backend().buffer()[(row.x + crate::launcher::PAD_X - 1, y)];
                assert_eq!(bar.bg, accent, "line {y}");
            }
        });
    }

    #[test]
    fn a_wide_title_keeps_the_age_in_its_column() {
        with_default_config(|| {
            let mut app = app(true);
            seed_board(&mut app);
            upsert(
                &mut app,
                Entity::Agent(agent_row(
                    "a5",
                    "w2",
                    "トークンの保存先をデータベースに移す",
                    AgentStatus::Running,
                    AgentRole::Worker,
                )),
            );
            draw_at(&mut app, 100, 30);
            f10(&mut app);
            let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
            terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
            let buffer = terminal.backend().buffer();
            let seam = crate::board::width(100) - 1;
            let text = buffer_text(&terminal);
            assert!(
                buffer.content().iter().any(|c| c.symbol() == "ト"),
                "{text}"
            );
            for y in 3..27 {
                assert_eq!(buffer[(seam, y)].symbol(), "│", "line {y}: {text}");
            }
            let (row, _) = app
                .chrome
                .hits
                .iter()
                .find(|(_, h)| *h == HitTarget::OrchestratorChip(agent("a5")))
                .cloned()
                .unwrap();
            let age: String = (seam - 4..seam - 1)
                .map(|x| buffer[(x, row.y)].symbol())
                .collect();
            assert_eq!(age, "now", "the age keeps its column: {text}");
        });
    }

    #[test]
    fn a_click_on_a_board_row_shows_it_and_the_crumb_goes_back() {
        with_default_config(|| {
            let mut app = app(true);
            seed_board(&mut app);
            draw_wide(&mut app);
            f10(&mut app);
            draw_wide(&mut app);
            assert!(
                !app.chrome
                    .hits
                    .iter()
                    .any(|(_, h)| *h == HitTarget::OrchestratorChip(agent("a1"))),
                "the idle one is counted, not a row"
            );
            click(&mut app, HitTarget::OrchestratorChip(agent("a4")));
            assert_eq!(showing(&app), Some(agent("a4")));
            assert!(app.pane.term_locked, "the keys go to it");
            draw_wide(&mut app);
            click(&mut app, HitTarget::OrchestratorChip(agent("o1")));
            assert!(app.orchestrator_view());
            assert_eq!(showing(&app), Some(agent("o1")));
        });
    }

    #[test]
    fn a_board_too_long_for_the_screen_counts_what_it_left_off() {
        with_default_config(|| {
            let mut app = app(true);
            for i in 0..12 {
                upsert(
                    &mut app,
                    Entity::Agent(agent_row(
                        &format!("r{i}"),
                        "w1",
                        &format!("running {i}"),
                        AgentStatus::Running,
                        AgentRole::Worker,
                    )),
                );
            }
            draw_at(&mut app, 120, 14);
            f10(&mut app);
            let text = draw_at(&mut app, 120, 14);
            let column = board_column(&text);
            let rows = column.iter().filter(|l| l.contains("● demo")).count();
            assert_eq!(
                column.iter().find(|l| l.contains("more")).map(|l| l.trim()),
                Some(format!("+{} more · 1 idle", 12 - rows).as_str()),
                "{text}"
            );
            assert!(rows > 0, "{text}");

            // F9 down past the last row drawn: the board moves along, the
            // lit row always on it and the rest still counted.
            for _ in 0..12 {
                f9(&mut app, KeyModifiers::NONE);
                let text = draw_at(&mut app, 120, 14);
                let on = showing(&app).unwrap();
                assert!(
                    app.chrome
                        .hits
                        .iter()
                        .any(|(_, h)| *h == HitTarget::OrchestratorChip(on.clone())),
                    "{on:?} is drawn: {text}"
                );
                let column = board_column(&text);
                let rows = column.iter().filter(|l| l.contains("● demo")).count();
                let counts = column
                    .iter()
                    .find(|l| l.contains("above") || l.contains("more"))
                    .map(|l| l.trim().to_string())
                    .unwrap_or_default();
                let left_off: usize = counts
                    .split(" · ")
                    .filter_map(|part| {
                        part.strip_prefix('+')?
                            .split(' ')
                            .next()?
                            .parse::<usize>()
                            .ok()
                    })
                    .sum();
                assert_eq!(left_off, 12 - rows, "{text}");
                assert!(counts.ends_with("· 1 idle"), "{text}");
            }
        });
    }

    #[test]
    fn the_board_reads_a_row_once_per_state_and_says_why_under_it() {
        with_default_config(|| {
            let mut app = app(true);
            seed_asking(&mut app);
            draw_wide(&mut app);
            f10(&mut app);
            draw_wide(&mut app);
            assert_eq!(app.pane.board_cards, [AgentId("a2".into())]);
            let ask = |app: &mut App| {
                let mut out = Vec::new();
                super::super::request_board_reads(app, &mut out);
                out
            };
            assert!(
                ask(&mut app).is_empty(),
                "the first beat only notes the state"
            );
            let out = ask(&mut app);
            let req_id = match out.as_slice() {
                [ClientRequest::TailOutput {
                    req_id,
                    session,
                    after_seq: None,
                    ..
                }] if *session == agent("a2") => *req_id,
                other => panic!("one read of a2: {other:?}"),
            };
            assert!(ask(&mut app).is_empty(), "read once per state");
            server(
                &mut app,
                ServerEvent::OutputTail {
                    req_id,
                    session: agent("a2"),
                    tail: Some(nebula_core::OutputTail {
                        cols: 40,
                        rows: 12,
                        end_seq: 9,
                        data: [
                            "❯ go",
                            "",
                            "────────────────────────────────────────",
                            " Bash command",
                            "╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌",
                            " cargo test --workspace",
                            "╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌",
                            " Do you want to proceed?",
                            " ❯ 1. Yes",
                            "   2. No",
                        ]
                        .join("\r\n")
                        .into_bytes(),
                    }),
                },
            );
            let text = draw_wide(&mut app);
            assert!(
                board_column(&text)
                    .iter()
                    .any(|l| l.trim() == "Bash: cargo test --workspace"),
                "{text}"
            );

            // The line outlasts the board: back on the grid and in again,
            // it is there on the first frame, with nothing asked again.
            f10(&mut app);
            draw_wide(&mut app);
            assert!(ask(&mut app).is_empty());
            f10(&mut app);
            let text = draw_wide(&mut app);
            assert!(text.contains("Bash: cargo test --workspace"), "{text}");
            assert!(ask(&mut app).is_empty());

            // A new state is a new read; the old line does not stand in.
            let mut again = agent_row(
                "a2",
                "w1",
                "asking",
                AgentStatus::NeedsFeedback,
                AgentRole::Worker,
            );
            again.status_changed_at += 1;
            upsert(&mut app, Entity::Agent(again.clone()));
            let text = draw_wide(&mut app);
            assert!(!text.contains("cargo test"), "{text}");
            assert!(ask(&mut app).is_empty());
            assert_eq!(ask(&mut app).len(), 1);

            // One that began beats ago, first seen now, is read at once.
            again.status_changed_at -= 60_000;
            upsert(&mut app, Entity::Agent(again));
            draw_wide(&mut app);
            assert_eq!(ask(&mut app).len(), 1);
        });
    }

    #[test]
    fn f10_puts_the_orchestrator_up_and_leaves_the_grid_where_it_was() {
        with_default_config(|| {
            let mut app = app(true);
            f10(&mut app);
            assert!(app.orchestrator_view());
            assert_eq!(showing(&app), Some(agent("o1")));
            assert!(app.pane.term_locked, "the keys go to the chat");
            assert_eq!(cursor(&app), Some(agent("a1")), "the grid did not move");
            let text = draw(&mut app);
            assert!(
                text.contains("‹ sessions / ● orchestrator  ● agent-1"),
                "{text}"
            );

            // F10 again, from inside the locked chat: out to the grid, the
            // pane back on the card under the cursor.
            f10(&mut app);
            assert!(!app.orchestrator_view());
            assert!(!app.pane.collapsed);
            assert_eq!(showing(&app), Some(agent("a1")));
        });
    }

    /// The narrow view's strip keeps the session on screen drawn: past
    /// what the row has room for, it scrolls, counting the chips it passed
    /// after the orchestrator's. A captain's chip names its project, and a
    /// name cut short ends on its ellipsis, never a space before it.
    #[test]
    fn the_strip_scrolls_to_the_session_on_screen() {
        with_default_config(|| {
            let mut app = app(true);
            seed_board(&mut app);
            upsert(
                &mut app,
                Entity::Agent(agent_row(
                    "c1",
                    "w2",
                    "captain",
                    AgentStatus::Running,
                    AgentRole::Captain,
                )),
            );
            for (id, name) in [
                ("b1", "Fix navbar overflow on mobile"),
                ("b2", "Dark mode toggle"),
                ("b3", "Upgrade React"),
                ("b4", "Cache warmup"),
            ] {
                upsert(
                    &mut app,
                    Entity::Agent(agent_row(
                        id,
                        "w1",
                        name,
                        AgentStatus::Running,
                        AgentRole::Worker,
                    )),
                );
            }
            f10(&mut app);
            let head = |text: &str| text.lines().nth(1).unwrap_or_default().to_string();
            let text = draw(&mut app);
            assert!(head(&text).contains("Fix navbar overflow on…"), "{text}");
            assert!(!head(&text).contains(" …"), "{text}");
            let last = app.strip_sessions().last().cloned().unwrap();
            let name = app
                .tree
                .agents
                .iter()
                .find(|a| a.id == last)
                .unwrap()
                .name
                .clone();
            assert!(!head(&text).contains(&name), "off the end at first: {text}");
            super::show_session(&mut app, SessionRef::Agent(last), &mut Vec::new());
            let text = draw(&mut app);
            let line = head(&text);
            assert!(line.contains(&name), "the lit chip is drawn: {text}");
            assert!(line.contains("● orchestrator  +"), "{text}");
            assert!(line.chars().count() <= 90, "{text}");
            super::show_session(&mut app, agent("c1"), &mut Vec::new());
            let text = draw(&mut app);
            assert!(head(&text).contains("● api-server captain"), "{text}");
        });
    }

    /// A board taller than the screen scrolls under the wheel, a row a
    /// notch, without touching the chat; `F9` brings the lit row back.
    #[test]
    fn the_wheel_scrolls_a_board_taller_than_the_screen() {
        with_default_config(|| {
            let mut app = app(true);
            seed_board(&mut app);
            for i in 0..8 {
                upsert(
                    &mut app,
                    Entity::Agent(agent_row(
                        &format!("r{i}"),
                        "w1",
                        &format!("Running {i}"),
                        AgentStatus::Running,
                        AgentRole::Worker,
                    )),
                );
            }
            f10(&mut app);
            let first_row = |text: &str| {
                board_column(text)
                    .into_iter()
                    .find(|l| l.trim_start().starts_with('●'))
                    .unwrap_or_default()
            };
            let text = draw_at(&mut app, 120, 16);
            assert!(first_row(&text).contains("asking"), "{text}");
            assert!(text.contains("more"), "{text}");
            let wheel = |app: &mut App, kind| {
                let (rect, _) = app
                    .chrome
                    .hits
                    .iter()
                    .find(|(_, h)| *h == HitTarget::Board)
                    .cloned()
                    .expect("the board is drawn");
                let event = MouseEvent {
                    kind,
                    column: rect.x + 1,
                    row: rect.y + rect.height - 1,
                    modifiers: KeyModifiers::NONE,
                };
                let mut out = Vec::new();
                handle_terminal_event(app, crossterm::event::Event::Mouse(event), &mut out);
                out
            };
            for _ in 0..3 {
                let out = wheel(&mut app, MouseEventKind::ScrollDown);
                assert!(out.is_empty(), "nothing reaches the chat: {out:?}");
                draw_at(&mut app, 120, 16);
            }
            let text = draw_at(&mut app, 120, 16);
            assert!(!first_row(&text).contains("asking"), "{text}");
            assert!(text.contains("+3 above · +"), "{text}");
            for _ in 0..20 {
                wheel(&mut app, MouseEventKind::ScrollDown);
                draw_at(&mut app, 120, 16);
            }
            let text = draw_at(&mut app, 120, 16);
            assert!(
                text.contains("Running"),
                "no further than the last rows: {text}"
            );
            for _ in 0..30 {
                wheel(&mut app, MouseEventKind::ScrollUp);
                draw_at(&mut app, 120, 16);
            }
            let text = draw_at(&mut app, 120, 16);
            assert!(first_row(&text).contains("asking"), "{text}");
            wheel(&mut app, MouseEventKind::ScrollDown);
            f9(&mut app, KeyModifiers::NONE);
            assert_eq!(app.launcher.board_scroll, None);
        });
    }

    /// A footer with no room for its whole trail cuts it on an ellipsis,
    /// never halfway through a word, and with the orchestrator on screen
    /// the trail is its name alone.
    #[test]
    fn the_footer_trail_ends_on_an_ellipsis_when_cut() {
        with_default_config(|| {
            let mut app = app(true);
            upsert(
                &mut app,
                Entity::Agent(agent_row(
                    "g1",
                    "w1",
                    "Add Contributing Guide To The Repository",
                    AgentStatus::Running,
                    AgentRole::Worker,
                )),
            );
            f10(&mut app);
            let footer = |app: &mut App, w: u16| {
                let text = draw_at(app, w, 20);
                text.lines().last().unwrap_or_default().to_string()
            };
            let line = footer(&mut app, 90);
            assert!(line.contains("·  orchestrator    ^q: back"), "{line}");
            super::show_session(&mut app, agent("g1"), &mut Vec::new());
            for w in [70, 80, 90] {
                let line = footer(&mut app, w);
                let left = line.split("  1 agent").next().unwrap_or_default();
                let left = left
                    .split("  2 agents")
                    .next()
                    .unwrap_or_default()
                    .trim_end();
                assert!(
                    left.ends_with('…') || left.ends_with("Repository") || left.ends_with("back"),
                    "{w}: {line:?}"
                );
            }
        });
    }

    /// Out of the view onto a project with no session in it, the pane is
    /// empty: it never stays on the orchestrator the view was showing.
    #[test]
    fn leaving_the_view_onto_an_empty_project_empties_the_pane() {
        with_default_config(|| {
            let mut app = app(true);
            app.tree.agents.retain(|a| a.id.0 != "a1");
            draw(&mut app);
            assert_eq!(cursor(&app), None);
            f10(&mut app);
            assert_eq!(showing(&app), Some(agent("o1")));
            let out = f10(&mut app);
            assert!(!app.orchestrator_view());
            assert_eq!(showing(&app), None);
            assert!(
                out.iter().any(
                    |r| matches!(r, ClientRequest::Detach { session } if *session == agent("o1"))
                ),
                "{out:?}"
            );
        });
    }

    #[test]
    fn no_list_of_projects_shows_the_home() {
        with_default_config(|| {
            let mut app = app(true);
            let rows: Vec<_> = app
                .project_rows()
                .iter()
                .map(|i| app.tree.projects[*i].name.clone())
                .collect();
            assert_eq!(rows, ["demo"]);
            let tabs: Vec<_> = crate::launcher::project_tabs(&app)
                .into_iter()
                .map(|t| t.name)
                .collect();
            assert_eq!(tabs, ["demo"]);
            // The header's button is the one place the grid names it.
            let text = draw(&mut app);
            assert_eq!(text.matches("orchestrator").count(), 1, "{text}");
            assert!(text.contains("orchestrator F10"), "{text}");
            // A jump to its session, as the palette or an alert makes one,
            // comes up in the view instead of looking for a grid.
            jump_to_target(
                &mut app,
                PaletteTarget::Session(AgentId("o1".into())),
                Landing::Attach,
                &mut Vec::new(),
            );
            assert!(app.orchestrator_view());
            assert_eq!(showing(&app), Some(agent("o1")));
        });
    }

    #[test]
    fn a_session_from_the_strip_comes_back_to_the_orchestrator() {
        with_default_config(|| {
            let mut app = app(true);
            f10(&mut app);
            draw(&mut app);
            click(&mut app, HitTarget::OrchestratorChip(agent("a1")));
            assert!(app.orchestrator_view());
            assert_eq!(showing(&app), Some(agent("a1")));
            let text = draw(&mut app);
            assert!(
                text.contains("‹ sessions / ● orchestrator  ● agent-1"),
                "{text}"
            );

            hatch(&mut app);
            assert!(app.orchestrator_view());
            assert_eq!(showing(&app), Some(agent("o1")));
            hatch(&mut app);
            assert!(!app.orchestrator_view());
        });
    }

    #[test]
    fn the_crumb_goes_all_the_way_out_to_the_grid() {
        with_default_config(|| {
            let mut app = app(true);
            f10(&mut app);
            draw(&mut app);
            click(&mut app, HitTarget::OrchestratorChip(agent("a1")));
            draw(&mut app);
            click(&mut app, HitTarget::LauncherCrumb);
            assert!(!app.orchestrator_view());
            assert!(!app.pane.collapsed);
            assert_eq!(cursor(&app), Some(agent("a1")));
        });
    }

    #[test]
    fn the_header_button_is_f10() {
        with_default_config(|| {
            let mut app = app(true);
            click(&mut app, HitTarget::LauncherOrchestrator);
            assert!(app.orchestrator_view());
            assert_eq!(showing(&app), Some(agent("o1")));
            assert_eq!(cursor(&app), Some(agent("a1")), "the grid did not move");

            // With no orchestrator yet, the click starts one, as F10 does.
            let mut app = self::app(false);
            let out = click(&mut app, HitTarget::LauncherOrchestrator);
            assert!(
                out.iter()
                    .any(|r| matches!(r, ClientRequest::AddProject { .. })),
                "{out:?}"
            );
        });
    }

    #[test]
    fn a_row_moving_under_the_cursor_leaves_the_view_alone() {
        with_default_config(|| {
            let mut app = app(true);
            seed_asking(&mut app);
            f10(&mut app);
            let mut archived =
                agent_row("a1", "w1", "agent-1", AgentStatus::Fresh, AgentRole::Worker);
            archived.archived = true;
            let out = upsert(&mut app, Entity::Agent(archived));
            assert!(app.orchestrator_view());
            assert_eq!(showing(&app), Some(agent("o1")));
            assert!(
                !out.iter()
                    .any(|r| matches!(r, ClientRequest::Attach { .. })),
                "{out:?}"
            );
        });
    }

    #[test]
    fn the_first_f10_makes_the_home_and_starts_the_orchestrator_there() {
        with_default_config(|| {
            let mut app = app(false);
            let out = f10(&mut app);
            let req_id = out
                .iter()
                .find_map(|r| match r {
                    ClientRequest::AddProject {
                        req_id,
                        path,
                        create_missing,
                        ..
                    } => {
                        assert_eq!(path, crate::app::orchestrator_home());
                        assert!(create_missing, "the folder is made when missing");
                        Some(*req_id)
                    }
                    _ => None,
                })
                .expect("the home added");
            assert!(f10(&mut app).is_empty(), "a second F10 waits for the first");

            // The Ack can beat the checkout's upsert: the create waits for
            // the checkout.
            let out = server(
                &mut app,
                ServerEvent::Ack {
                    req_id,
                    created: Some(EntityId::Project(ProjectId("ph".into()))),
                },
            );
            assert!(out.is_empty(), "{out:?}");
            let out = seed_home(&mut app);
            let req_id = out
                .iter()
                .find_map(|r| match r {
                    ClientRequest::CreateAgent {
                        req_id,
                        worktree,
                        role,
                        auto_title,
                        ..
                    } => {
                        assert_eq!(worktree, &WorktreeId("wh".into()));
                        assert_eq!(*role, AgentRole::Orchestrator);
                        assert!(!auto_title, "no `nebula rename` for the orchestrator");
                        Some(*req_id)
                    }
                    _ => None,
                })
                .expect("the orchestrator created in the home");

            upsert(
                &mut app,
                Entity::Agent(agent_row(
                    "o9",
                    "wh",
                    "orchestrator",
                    AgentStatus::Fresh,
                    AgentRole::Orchestrator,
                )),
            );
            server(
                &mut app,
                ServerEvent::Ack {
                    req_id,
                    created: Some(EntityId::Agent(AgentId("o9".into()))),
                },
            );
            assert!(app.orchestrator_view());
            assert_eq!(showing(&app), Some(agent("o9")));
            assert!(app.pane.term_locked, "the keys go to it");
            assert_eq!(cursor(&app), Some(agent("a1")), "the grid did not move");
            assert_eq!(app.chrome.flash, None, "no longer starting");

            // Claude opens on its question whether to trust the home: the
            // footer says what it asks and how to answer it.
            server(
                &mut app,
                ServerEvent::StatusChanged {
                    agent: AgentId("o9".into()),
                    status: AgentStatus::NeedsFeedback,
                    changed_at: 1,
                    unseen: false,
                },
            );
            assert_eq!(
                app.chrome.flash.as_deref(),
                Some(super::super::orchestrator::FIRST_QUESTION)
            );
        });
    }

    #[test]
    fn the_orchestrators_review_shows_the_session_in_the_view() {
        with_default_config(|| {
            let mut app = app(true);
            hse(
                &mut app,
                ServerEvent::ReviewOpened {
                    opener: AgentId("o1".into()),
                    sessions: vec![agent("a1")],
                    tabs: Vec::new(),
                },
            );
            assert!(app.orchestrator_view());
            assert_eq!(showing(&app), Some(agent("a1")));
            assert!(app.modals.overlay.is_none());
            hatch(&mut app);
            assert_eq!(showing(&app), Some(agent("o1")));
        });
    }

    #[test]
    fn f9_walks_the_strip_from_inside_the_chat() {
        with_default_config(|| {
            let mut app = app(true);
            seed_asking(&mut app);
            f10(&mut app);
            let mut sent = f9(&mut app, KeyModifiers::NONE);
            assert_eq!(showing(&app), Some(agent("a2")), "the one asking first");
            sent.extend(f9(&mut app, KeyModifiers::NONE));
            assert_eq!(showing(&app), Some(agent("a2")), "and only it");
            moved(&mut app, "a2", AgentStatus::Running);
            sent.extend(f9(&mut app, KeyModifiers::NONE));
            assert_eq!(showing(&app), Some(agent("a1")), "then the whole strip");
            sent.extend(f9(&mut app, KeyModifiers::NONE));
            assert_eq!(showing(&app), Some(agent("a2")), "round to the head");
            sent.extend(f9(&mut app, KeyModifiers::SHIFT));
            assert_eq!(showing(&app), Some(agent("a1")), "⇧F9 walks back");
            assert!(
                !sent
                    .iter()
                    .any(|r| matches!(r, ClientRequest::Input { .. })),
                "F9 is nebula's, never the agent's: {sent:?}"
            );
            assert!(app.orchestrator_view());
            hatch(&mut app);
            assert_eq!(showing(&app), Some(agent("o1")));
        });
    }

    #[test]
    fn f9_from_the_grid_brings_the_view_up_on_the_strips_head() {
        with_default_config(|| {
            let mut app = app(true);
            seed_asking(&mut app);
            f9(&mut app, KeyModifiers::NONE);
            assert!(app.orchestrator_view());
            assert_eq!(showing(&app), Some(agent("a2")));
            f10(&mut app);
            assert_eq!(cursor(&app), Some(agent("a1")), "the grid did not move");
        });
    }

    /// `F9` on the only session it walks says there is no other, rather
    /// than put the same one up again without a word.
    #[test]
    fn f9_on_the_only_session_to_walk_says_so() {
        with_default_config(|| {
            let mut app = app(true);
            seed_board(&mut app);
            for id in ["a3", "a4"] {
                app.tree.agents.retain(|a| a.id.0 != id);
            }
            draw_wide(&mut app);
            f10(&mut app);
            f9(&mut app, KeyModifiers::NONE);
            let only = app.walk_ring();
            assert_eq!(only.len(), 1, "{only:?}");
            assert_eq!(showing(&app), Some(SessionRef::Agent(only[0].clone())));
            assert_eq!(app.chrome.flash, None);
            f9(&mut app, KeyModifiers::NONE);
            assert_eq!(showing(&app), Some(SessionRef::Agent(only[0].clone())));
            assert_eq!(
                app.chrome.flash.as_deref(),
                Some("no other session to show")
            );
        });
    }

    #[test]
    fn f9_with_no_other_session_stays_put() {
        with_default_config(|| {
            let mut app = App::new();
            seed_home(&mut app);
            upsert(
                &mut app,
                Entity::Agent(agent_row(
                    "o1",
                    "wh",
                    "orchestrator",
                    AgentStatus::Running,
                    AgentRole::Orchestrator,
                )),
            );
            seed_tree(&mut app);
            let mut gone = agent_row("a1", "w1", "agent-1", AgentStatus::Fresh, AgentRole::Worker);
            gone.archived = true;
            upsert(&mut app, Entity::Agent(gone));
            f10(&mut app);
            f9(&mut app, KeyModifiers::NONE);
            assert_eq!(showing(&app), Some(agent("o1")));
            assert_eq!(
                app.chrome.flash.as_deref(),
                Some("no other session to show")
            );
        });
    }

    #[test]
    fn the_view_survives_a_relaunch() {
        with_default_config(|| {
            let mut app = app(true);
            f10(&mut app);
            draw(&mut app);
            click(&mut app, HitTarget::OrchestratorChip(agent("a1")));
            let (mut next, out) = relaunched(&app);
            assert!(next.orchestrator_view());
            assert_eq!(showing(&next), Some(agent("a1")));
            assert!(next.pane.term_locked);
            assert!(
                out.iter().any(|r| matches!(r, ClientRequest::Attach { session, .. } if *session == agent("a1")))
                    || next.pane.pending_attach.is_some(),
                "a1 attaches: {out:?}"
            );
            hatch(&mut next);
            assert_eq!(showing(&next), Some(agent("o1")));
        });
    }

    #[test]
    fn a_relaunch_with_the_orchestrator_gone_lands_on_the_grid() {
        with_default_config(|| {
            let mut app = app(true);
            f10(&mut app);
            let json = ui_state_json(&app);
            let mut tree = app.tree.clone();
            tree.agents.retain(|a| a.id.0 != "o1");
            let mut next = App::new();
            server(
                &mut next,
                ServerEvent::Snapshot {
                    projects: tree.projects,
                    worktrees: tree.worktrees,
                    agents: tree.agents,
                    terminals: tree.terminals,
                    links: tree.links,
                    pr_seen: Vec::new(),
                    ui_state: Some(json),
                },
            );
            assert!(!next.orchestrator_view());
            assert!(!next.pane.collapsed);
            assert_eq!(showing(&next), Some(agent("a1")));
        });
    }

    /// `F8` hands the keys to the BOARD: `j`/`k` move its highlight, no
    /// key reaches the session meanwhile, Enter shows the row's session
    /// and hands it the keys, and Esc or `F8` hands them back unmoved. A
    /// screen with no board has nothing to take them.
    #[test]
    fn f8_hands_the_keys_to_the_board() {
        with_default_config(|| {
            let mut app = app(true);
            seed_board(&mut app);
            f10(&mut app);
            draw(&mut app);
            let out = key(&mut app, KeyCode::F(8), KeyModifiers::NONE);
            assert!(out.is_empty());
            assert_eq!(app.launcher.board_focus, None, "no board at 90 columns");
            app.chrome.flash = None;

            draw_wide(&mut app);
            key(&mut app, KeyCode::F(8), KeyModifiers::NONE);
            let rows: Vec<AgentId> = crate::board::board(&app)
                .rows
                .iter()
                .map(|r| r.id.clone())
                .collect();
            assert_eq!(app.launcher.board_focus.as_ref(), Some(&rows[0]));
            let text = draw_wide(&mut app);
            assert!(text.contains("j/k: move  Enter: show"), "{text}");

            let out = key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
            assert!(out.is_empty(), "nothing is typed into the chat: {out:?}");
            assert_eq!(app.launcher.board_focus.as_ref(), Some(&rows[1]));
            key(&mut app, KeyCode::Char('k'), KeyModifiers::NONE);
            key(&mut app, KeyCode::Char('k'), KeyModifiers::NONE);
            assert_eq!(
                app.launcher.board_focus.as_ref(),
                Some(&rows[0]),
                "it stops at the first row"
            );
            let out = key(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
            assert!(out.is_empty(), "{out:?}");

            key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
            assert_eq!(app.launcher.board_focus, None);
            assert_eq!(showing(&app), Some(agent("o1")), "the chat stayed up");

            key(&mut app, KeyCode::F(8), KeyModifiers::NONE);
            key(&mut app, KeyCode::Down, KeyModifiers::NONE);
            key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
            assert_eq!(showing(&app), Some(SessionRef::Agent(rows[1].clone())));
            assert_eq!(app.launcher.board_focus, None, "the session has the keys");
            assert!(app.orchestrator_view());
        });
    }

    /// The layout is saved when it changes, not only on a quit, so a
    /// closed window comes back in the view it was left in.
    #[test]
    fn a_changed_layout_is_saved_once() {
        with_default_config(|| {
            let mut app = app(true);
            app.chrome.snapshot_loaded = true;
            let mut saved = None;
            assert!(super::super::changed_ui_state(&app, &mut saved).is_some());
            assert_eq!(super::super::changed_ui_state(&app, &mut saved), None);
            f10(&mut app);
            let json = super::super::changed_ui_state(&app, &mut saved).unwrap();
            assert!(json.contains("\"orchestrator\":{"), "{json}");
            assert_eq!(super::super::changed_ui_state(&app, &mut saved), None);
            assert_eq!(
                super::super::changed_ui_state(&App::new(), &mut None),
                None,
                "no layout before the first Snapshot"
            );
        });
    }

    /// START IN THE ORCHESTRATOR: a launch that left the grid up comes
    /// back in the view, on the orchestrator, or starting one when there is
    /// none; with the setting off it comes back on the grid.
    #[test]
    fn a_launch_opens_on_the_orchestrator_with_the_setting_on() {
        with_config_json(r#"{"start_in_orchestrator": true}"#, || {
            let (next, _) = relaunched(&app(true));
            assert!(next.orchestrator_view());
            assert_eq!(showing(&next), Some(agent("o1")));

            let (next, out) = relaunched(&app(false));
            assert!(!next.orchestrator_view());
            assert!(
                out.iter().any(|r| matches!(
                    r,
                    ClientRequest::AddProject { path, .. } if path == crate::app::orchestrator_home()
                )),
                "the orchestrator's home is asked for: {out:?}"
            );
        });
        with_default_config(|| {
            let (next, _) = relaunched(&app(true));
            assert!(!next.orchestrator_view());
        });
    }

    /// A session in a WORKTREE of its own names its branch and pull
    /// request on a line under its title; one in the root checkout with no
    /// pull request has no such line.
    #[test]
    fn a_row_names_its_worktrees_branch_and_pull_request() {
        with_default_config(|| {
            let mut app = app(true);
            seed_board(&mut app);
            upsert(
                &mut app,
                Entity::Worktree(Worktree {
                    id: WorktreeId("w3".into()),
                    project_id: ProjectId("p2".into()),
                    path: "/tmp/api-server-token-store".into(),
                    branch: "token-store".into(),
                    is_main: false,
                    sort_order: 1,
                    removed: false,
                }),
            );
            upsert(
                &mut app,
                Entity::Agent(agent_row(
                    "a5",
                    "w3",
                    "Sqlite migration",
                    AgentStatus::Running,
                    AgentRole::Worker,
                )),
            );
            app.github.pull_requests.insert(
                WorktreeId("w3".into()),
                Some(crate::pull_request::PullRequest {
                    number: 57,
                    url: "https://github.com/o/r/pull/57".into(),
                    title: "Move the token store to sqlite".into(),
                    state: crate::pull_request::STATE_OPEN.into(),
                    is_draft: false,
                    head: String::new(),
                    head_sha: String::new(),
                    base: "main".into(),
                    health: Default::default(),
                    activity: Vec::new(),
                }),
            );
            f10(&mut app);
            let column = board_column(&draw_wide(&mut app));
            let at = column
                .iter()
                .position(|l| l.contains("Sqlite migration"))
                .unwrap_or_else(|| panic!("the row is on the board: {column:#?}"));
            assert_eq!(column[at + 1], "    token-store  ↗ #57", "{column:#?}");
            let root = column
                .iter()
                .position(|l| l.contains("Token store on sqlite"))
                .unwrap();
            assert!(
                !column[root + 1].contains("main"),
                "the root checkout is not named: {column:#?}"
            );
            // The line is the row's: a click on it shows the session.
            let hit = HitTarget::OrchestratorChip(agent("a5"));
            let rect = app
                .chrome
                .hits
                .iter()
                .find(|(_, h)| *h == hit)
                .map(|(r, _)| *r)
                .unwrap();
            assert_eq!(rect.height, 2, "title and checkout");
        });
    }

    /// Land a board read of `id`'s screen, `lines` top to bottom, asked
    /// for the state it is in now.
    fn land_read(app: &mut App, id: &str, lines: &[&str]) {
        let mut out = Vec::new();
        super::super::request_board_reads(app, &mut out);
        super::super::request_board_reads(app, &mut out);
        let req_id = out
            .iter()
            .find_map(|r| match r {
                ClientRequest::TailOutput {
                    req_id, session, ..
                } if *session == agent(id) => Some(*req_id),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{id} was not read: {out:?}"));
        server(
            app,
            ServerEvent::OutputTail {
                req_id,
                session: agent(id),
                tail: Some(nebula_core::OutputTail {
                    cols: 60,
                    rows: lines.len() as u16,
                    end_seq: 1,
                    data: lines.join("\r\n").into_bytes(),
                }),
            },
        );
    }

    fn typed(out: &[ClientRequest]) -> Vec<(SessionRef, String)> {
        out.iter()
            .filter_map(|r| match r {
                ClientRequest::Input { session, data } if data != b"\r" => {
                    Some((session.clone(), String::from_utf8_lossy(data).into_owned()))
                }
                _ => None,
            })
            .collect()
    }

    /// A client that comes up after a finish was read finds out whether
    /// it asked: the board reads it once, and it is back under `needs you`.
    #[test]
    fn a_read_finish_that_asked_is_found_again_after_a_relaunch() {
        with_default_config(|| {
            let mut app = app(true);
            seed_board(&mut app);
            for a in &mut app.tree.agents {
                a.unseen = false;
                a.status_changed_at -= 60_000;
            }
            f10(&mut app);
            let (mut next, _) = relaunched(&app);
            let text = draw_wide(&mut next);
            assert_eq!(board_column(&text)[0].trim(), "1 needs you · 1 running");
            land_read(&mut next, "a3", &["⏺ QUESTION: per token or per IP?"]);
            let text = draw_wide(&mut next);
            assert_eq!(
                board_column(&text)[0].trim(),
                "2 need you · 1 running",
                "{text}"
            );
            assert!(text.contains("QUESTION: per token or per IP?"), "{text}");
            // Read once: it is not asked after again.
            let mut out = Vec::new();
            super::super::request_board_reads(&mut next, &mut out);
            super::super::request_board_reads(&mut next, &mut out);
            assert!(out.is_empty(), "{out:?}");
        });
    }

    /// A finish that asks the user, or is blocked, waits on them as a
    /// dialog does: it is listed and counted under `needs you`, its line
    /// as it was, and stays there once read until its next turn. A plain
    /// `DONE` stays under `done`.
    #[test]
    fn a_finish_that_asks_is_listed_under_needs_you() {
        with_default_config(|| {
            let mut app = app(true);
            seed_board(&mut app);
            f10(&mut app);
            draw_wide(&mut app);
            land_read(&mut app, "a3", &["⏺ QUESTION: per token or per IP?"]);
            let text = draw_wide(&mut app);
            let column = board_column(&text);
            assert_eq!(column[0].trim(), "2 need you · 1 running", "{text}");
            let at = |needle: &str| {
                column
                    .iter()
                    .position(|l| l.trim_start().starts_with(needle))
            };
            assert!(at("● api-server  JSON") < at("running"), "{text}");
            assert!(!text.contains(" done"), "{text}");
            assert!(text.contains("QUESTION: per token or per IP?"), "{text}");
            assert_eq!(
                app.walk_ring()[..2],
                [AgentId("a2".into()), AgentId("a3".into())]
            );
            // Read, and walked past: it is still unanswered, so it stays.
            app.tree
                .agents
                .iter_mut()
                .find(|a| a.id.0 == "a3")
                .unwrap()
                .unseen = false;
            let text = draw_wide(&mut app);
            assert_eq!(
                board_column(&text)[0].trim(),
                "2 need you · 1 running",
                "{text}"
            );
            assert!(app.walk_ring().contains(&AgentId("a3".into())));

            let mut done = app
                .tree
                .agents
                .iter()
                .find(|a| a.id.0 == "a3")
                .unwrap()
                .clone();
            done.status_changed_at += 1;
            done.unseen = true;
            upsert(&mut app, Entity::Agent(done));
            draw_wide(&mut app);
            land_read(&mut app, "a3", &["⏺ DONE added the flag"]);
            let text = draw_wide(&mut app);
            assert_eq!(
                board_column(&text)[0].trim(),
                "1 needs you · 1 done · 1 running"
            );
        });
    }

    /// The answers a reply ends on (`NEXT: a | b`) are the chat's: the
    /// board says why with the report line and never lists them, and a
    /// function key nebula has no use for is the session's own.
    #[test]
    fn a_replys_answers_are_never_on_the_board() {
        with_default_config(|| {
            let mut app = app(true);
            seed_board(&mut app);
            f10(&mut app);
            draw_wide(&mut app);
            land_read(
                &mut app,
                "a3",
                &["⏺ DONE added the flag", "  NEXT: Open a PR | Add a test"],
            );
            let text = draw_wide(&mut app);
            let column = board_column(&text);
            let at = column.iter().position(|l| l.contains("JSON flag")).unwrap();
            assert_eq!(column[at + 1].trim(), "added the flag", "{text}");
            assert!(
                !column.iter().any(|l| l.contains("Open a PR")),
                "no answer on the board: {text}"
            );
            assert!(!text.contains("steps"), "and no key for them: {text}");
            super::show_session(&mut app, agent("a3"), &mut Vec::new());
            let out = key(&mut app, KeyCode::F(1), KeyModifiers::NONE);
            assert!(
                typed(&out).iter().all(|(_, text)| text != "Open a PR"),
                "F1 sends no answer: {out:?}"
            );
        });
    }

    #[test]
    fn a_full_screen_session_comes_back_full_screen() {
        with_default_config(|| {
            let mut app = app(false);
            key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
            assert!(app.pane.collapsed && !app.orchestrator_view());
            let (next, _) = relaunched(&app);
            assert!(next.pane.collapsed, "full-screen again");
            assert!(!next.orchestrator_view());
            assert_eq!(showing(&next), Some(agent("a1")));
        });
    }
}
