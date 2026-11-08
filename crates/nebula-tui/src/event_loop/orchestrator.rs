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
    match app.orchestrator_agent().map(|a| a.id.clone()) {
        Some(id) => show_session(app, SessionRef::Agent(id), out),
        None => start(app, out),
    }
    OPENED
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
/// under the grid's cursor, which the view never moved.
pub(super) fn leave(app: &mut App, out: &mut Vec<ClientRequest>) {
    leave_terminal_lock(app);
    if !app.launcher.launcher_unaimed {
        super::preview_selected_now(app, out);
    }
}

/// What `F9` and `⇧F9` say they did.
pub(super) const NEXT: &str = "Next session needing you";
pub(super) const PREVIOUS: &str = "Previous session needing you";

/// `F9` (`step` 1) and `⇧F9` (-1), from anywhere: the next session in the
/// strip after the one on screen, shown in the view. From the orchestrator,
/// or from outside the view, the walk starts at the strip's head, or its
/// tail going back, as `.` and `,` do.
pub(super) fn walk(app: &mut App, step: i64, out: &mut Vec<ClientRequest>) -> &'static str {
    let ring = app.strip_sessions();
    if ring.is_empty() {
        app.chrome.flash = Some("no other session to show".into());
        return if step > 0 { NEXT } else { PREVIOUS };
    }
    let showing = app
        .orchestrator_view()
        .then(|| app.pane.term.as_ref().map(|t| t.sref.clone()))
        .flatten();
    let at = match showing {
        Some(SessionRef::Agent(id)) => ring.iter().position(|r| *r == id),
        _ => None,
    };
    let len = ring.len() as i64;
    let next = match at {
        Some(i) => (i as i64 + step).rem_euclid(len),
        None if step > 0 => 0,
        None => len - 1,
    };
    show_session(app, SessionRef::Agent(ring[next as usize].clone()), out);
    if step > 0 {
        NEXT
    } else {
        PREVIOUS
    }
}

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
/// the user moved on meanwhile.
pub(super) fn created(app: &mut App, id: AgentId, follow: bool, out: &mut Vec<ClientRequest>) {
    if follow {
        show_session(app, SessionRef::Agent(id), out);
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{buffer_text, hse, press, seed_tree, with_default_config};
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
            status_changed_at: 0,
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

    fn draw(app: &mut App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|f| crate::ui::draw(f, app)).unwrap();
        buffer_text(&terminal)
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
            assert_eq!(cursor(&app), Some(agent("a1")), "the grid did not move");
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
            assert_eq!(showing(&app), Some(agent("a1")));
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
