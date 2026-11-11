//! The client's mirror of the DAEMON's entity tree, kept current by the
//! events it pushes ([`Tree::apply`]), and the orders every client reads
//! off it: the PALETTE's attention order of the sessions, and the BOARD's.

use nebula_core::{
    Agent, AgentId, AgentStatus, Entity, EntityId, Link, Project, ServerEvent, TerminalTab,
    Worktree, WorktreeId,
};
use std::cmp::Reverse;

/// A session that is mid-turn or blocked on the user. These count as
/// interacting *now*, so they head the sessions list however long the turn
/// has taken - the point is to keep what needs attention in view.
pub fn is_active_status(s: AgentStatus) -> bool {
    matches!(s, AgentStatus::Running | AgentStatus::NeedsFeedback)
}

/// When a session was last interacted with, as RECENCY ORDER sorts it: its
/// epoch-ms stamp, 0 for never run. A working session counts as
/// interacting *now*: it is producing output as you look at it, so it holds
/// the top of the list however long the turn has taken.
///
/// "Now" is later than every stamp rather than a reading of the clock, so
/// the order moves only when the tree does. With the clock, a turn the
/// DAEMON finished in the millisecond the TUI read it tied with the working
/// rows, led them on its stamp and dropped behind them a millisecond later,
/// and the cursors, row indices re-seated on each event, were left on
/// whatever row slid into their slot, another project's included.
pub fn last_interaction_ms(a: &Agent) -> i64 {
    if is_active_status(a.status) {
        i64::MAX
    } else {
        a.status_changed_at
    }
}

/// The tiers of the attention order, best first. A SESSION waiting on you
/// (NEEDS FEEDBACK) comes first, then one mid-turn (RUNNING), then one that
/// finished a turn nobody has read (UNSEEN); every other session sorts
/// under those in RECENCY ORDER.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AttentionTier {
    NeedsFeedback,
    Running,
    Unseen,
    Rest,
}

impl AttentionTier {
    /// The tier a session sorts into: its own status, read against the
    /// UNSEEN flag the DONE BADGE counts.
    pub fn of(a: &Agent) -> AttentionTier {
        match a.status {
            AgentStatus::NeedsFeedback => AttentionTier::NeedsFeedback,
            AgentStatus::Running => AttentionTier::Running,
            AgentStatus::Finished if a.unseen => AttentionTier::Unseen,
            _ => AttentionTier::Rest,
        }
    }
}

/// The ORCHESTRATOR HOME's folder: `orchestrator` in nebula's data
/// directory, which the daemon shares, resolved the way it stores a
/// project's path. Worked out once: the data directory exists by the time
/// any project is in the tree to compare with.
pub fn orchestrator_home() -> &'static std::path::Path {
    static HOME: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    HOME.get_or_init(|| {
        let data = nebula_core::paths::data_dir();
        std::fs::canonicalize(&data)
            .unwrap_or(data)
            .join("orchestrator")
    })
}

/// Replace the first entry of `list` that `same` pairs with `item`, or
/// append `item` when there is none.
pub fn upsert_by<T>(list: &mut Vec<T>, item: T, same: impl Fn(&T, &T) -> bool) {
    match list.iter_mut().find(|x| same(x, &item)) {
        Some(existing) => *existing = item,
        None => list.push(item),
    }
}

/// Client-side mirror of the entity tree: every project on this machine,
/// with the checkouts, sessions, terminals and links under them.
#[derive(Debug, Clone, Default)]
pub struct Tree {
    pub projects: Vec<Project>,
    pub worktrees: Vec<Worktree>,
    /// Worktrees whose checkout was deleted ([`Worktree::removed`]), kept
    /// for the archived sessions filed under them. Held apart from
    /// `worktrees` so nothing that lists, picks or polls a checkout ever
    /// meets one: only the ARCHIVED VIEW shows them.
    pub removed_worktrees: Vec<Worktree>,
    pub agents: Vec<Agent>,
    pub terminals: Vec<TerminalTab>,
    pub links: Vec<Link>,
    /// Whether any TUI is connected to the DAEMON, as it last said
    /// (`ServerEvent::TuiShowing`). A daemon older than
    /// [`nebula_core::SHOW_PROTOCOL`] never says, and this stays false.
    pub tui_open: bool,
    /// The session on screen in the TUI the user touched last, if any:
    /// what the web app's **Follow TUI** follows.
    pub tui_session: Option<AgentId>,
}

impl Tree {
    /// Fold one event into the mirror, as the TUI does into its own: a
    /// Snapshot replaces it, an upsert lands by id (a project in its
    /// `sort_order`, a checkout just deleted without the terminals that went
    /// with it), a removal takes the row and everything under it (the daemon
    /// cascades without saying so row by row).
    pub fn apply(&mut self, event: &ServerEvent) {
        match event {
            ServerEvent::Snapshot {
                projects,
                worktrees,
                agents,
                terminals,
                links,
                ..
            } => {
                // What the TUIs show is in no Snapshot: the daemon says it
                // again right after one, and until then it stands.
                *self = Tree {
                    projects: projects.clone(),
                    agents: agents.clone(),
                    terminals: terminals.clone(),
                    links: links.clone(),
                    tui_open: self.tui_open,
                    tui_session: self.tui_session.take(),
                    ..Tree::default()
                };
                self.set_worktrees(worktrees.clone());
            }
            ServerEvent::EntityUpserted { entity } => match entity.clone() {
                Entity::Project(p) => {
                    upsert_by(&mut self.projects, p, |x, y| x.id == y.id);
                    // Stable, so legacy all-zero ties keep their order.
                    self.projects.sort_by_key(|x| x.sort_order);
                }
                Entity::Worktree(w) => {
                    if w.removed {
                        self.terminals.retain(|t| t.worktree_id != w.id);
                    }
                    self.upsert_worktree(w);
                }
                Entity::Agent(a) => upsert_by(&mut self.agents, a, |x, y| x.id == y.id),
                Entity::Terminal(t) => upsert_by(&mut self.terminals, t, |x, y| x.id == y.id),
                Entity::Link(l) => upsert_by(&mut self.links, l, |x, y| x.id == y.id),
            },
            ServerEvent::EntityRemoved { id } => {
                match id {
                    EntityId::Project(id) => self.projects.retain(|p| &p.id != id),
                    EntityId::Worktree(id) => {
                        self.worktrees.retain(|w| &w.id != id);
                        self.removed_worktrees.retain(|w| &w.id != id);
                    }
                    EntityId::Agent(id) => self.agents.retain(|a| &a.id != id),
                    EntityId::Terminal(id) => self.terminals.retain(|t| &t.id != id),
                    EntityId::Link(id) => self.links.retain(|l| &l.id != id),
                }
                let Tree {
                    projects,
                    worktrees,
                    removed_worktrees,
                    agents,
                    terminals,
                    links,
                    ..
                } = self;
                let in_project = |w: &Worktree| projects.iter().any(|p| p.id == w.project_id);
                worktrees.retain(in_project);
                removed_worktrees.retain(in_project);
                let filed = |id: &WorktreeId| {
                    worktrees
                        .iter()
                        .chain(removed_worktrees.iter())
                        .any(|w| &w.id == id)
                };
                agents.retain(|a| filed(&a.worktree_id));
                terminals.retain(|t| filed(&t.worktree_id));
                links.retain(|l| filed(&l.worktree_id));
            }
            ServerEvent::StatusChanged {
                agent,
                status,
                changed_at,
                unseen,
            } => {
                if let Some(row) = self.agents.iter_mut().find(|a| &a.id == agent) {
                    row.status = *status;
                    row.status_changed_at = *changed_at;
                    row.unseen = *unseen;
                }
            }
            ServerEvent::TuiShowing { session, tui_open } => {
                self.tui_open = *tui_open;
                self.tui_session = session.clone();
            }
            _ => {}
        }
    }

    /// The worktree a session is filed under: its checkout, or the removed
    /// worktree an archived session outlived.
    pub fn filed_under(&self, id: &WorktreeId) -> Option<&Worktree> {
        self.worktrees
            .iter()
            .chain(&self.removed_worktrees)
            .find(|w| &w.id == id)
    }

    /// Take a Snapshot's worktrees: the checkouts, and the removed ones
    /// apart from them.
    pub fn set_worktrees(&mut self, worktrees: Vec<Worktree>) {
        let (removed, standing) = worktrees.into_iter().partition(|w| w.removed);
        self.worktrees = standing;
        self.removed_worktrees = removed;
    }

    /// Take a worktree row from the daemon: among the checkouts, or among
    /// the removed ones, never both.
    pub fn upsert_worktree(&mut self, w: Worktree) {
        let (list, other) = if w.removed {
            (&mut self.removed_worktrees, &mut self.worktrees)
        } else {
            (&mut self.worktrees, &mut self.removed_worktrees)
        };
        other.retain(|x| x.id != w.id);
        match list.iter_mut().find(|x| x.id == w.id) {
            Some(existing) => *existing = w,
            None => list.push(w),
        }
    }

    /// Any project at all? None is a first run: the splash, with the way
    /// to open one, is all there is to draw. The ORCHESTRATOR HOME is not
    /// one the user opened.
    pub fn has_projects(&self) -> bool {
        self.projects.iter().any(|p| !self.is_orchestrator_home(p))
    }

    /// Is `p` the ORCHESTRATOR HOME: nebula's own folder for the
    /// orchestrator, a project the daemon keeps like any other and no list
    /// of projects shows.
    pub fn is_orchestrator_home(&self, p: &Project) -> bool {
        p.repo_path == orchestrator_home()
    }

    /// The ORCHESTRATOR HOME's ROOT WORKTREE, once it is in the tree.
    pub fn orchestrator_home_root(&self) -> Option<&Worktree> {
        let home = self
            .projects
            .iter()
            .find(|p| self.is_orchestrator_home(p))?;
        self.worktrees
            .iter()
            .find(|w| w.project_id == home.id && w.is_main)
    }

    /// Does session `id` live in the ORCHESTRATOR HOME, where no grid shows
    /// a card for it?
    pub fn in_orchestrator_home(&self, id: &AgentId) -> bool {
        let Some(agent) = self.agents.iter().find(|a| &a.id == id) else {
            return false;
        };
        self.orchestrator_home_root()
            .is_some_and(|w| w.id == agent.worktree_id)
    }

    /// The project registered for the repo at `path`, or for any checkout
    /// of it - the folder the user named when a project already stands
    /// for it. An exact path match, so it is run on canonical paths (the
    /// TUI's add prompt canonicalizes first).
    pub fn project_at_path(&self, path: &std::path::Path) -> Option<&Project> {
        self.projects.iter().find(|p| {
            p.repo_path == path
                || self
                    .worktrees
                    .iter()
                    .any(|w| w.project_id == p.id && w.path == path)
        })
    }

    /// The ORCHESTRATOR session: the first live row the daemon persisted
    /// with that role. The daemon allows more than one; a view shows one.
    pub fn orchestrator_agent(&self) -> Option<&Agent> {
        self.agents
            .iter()
            .find(|a| a.role.is_orchestrator() && !a.archived)
    }

    /// Every live session in the PALETTE's attention order: by
    /// [`AttentionTier`], then most recently interacted with
    /// ([`last_interaction_ms`]), then tree order (each project in turn but
    /// the ORCHESTRATOR HOME, its worktrees, their sessions). Every project
    /// contributes, and archived sessions are left out: a released PTY has
    /// nothing left to ask of anyone.
    pub fn attention_sessions(&self) -> Vec<AgentId> {
        let mut sessions: Vec<&Agent> = Vec::new();
        for p in self
            .projects
            .iter()
            .filter(|p| !self.is_orchestrator_home(p))
        {
            for w in self.worktrees.iter().filter(|w| w.project_id == p.id) {
                sessions.extend(
                    self.agents
                        .iter()
                        .filter(|a| a.worktree_id == w.id && !a.archived),
                );
            }
        }
        // Stable: a tie keeps tree order.
        sessions.sort_by_key(|a| (AttentionTier::of(a), Reverse(last_interaction_ms(a))));
        sessions.into_iter().map(|a| a.id.clone()).collect()
    }

    /// The sessions the BOARD is built from: every live session but the
    /// orchestrator, in the attention order ([`Self::attention_sessions`]).
    pub fn board_sessions(&self) -> Vec<AgentId> {
        let orchestrator = self.orchestrator_agent().map(|a| a.id.clone());
        self.attention_sessions()
            .into_iter()
            .filter(|id| Some(id) != orchestrator.as_ref())
            .collect()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use nebula_core::{AgentKind, AgentRole, LinkId, ProjectId, TerminalId};
    use std::path::Path;

    fn worktree(id: &str, path: &Path, is_main: bool) -> Worktree {
        Worktree {
            id: WorktreeId(id.into()),
            project_id: ProjectId("p".into()),
            path: path.to_path_buf(),
            branch: id.into(),
            is_main,
            sort_order: 0,
            removed: false,
        }
    }

    fn project(path: &Path) -> Project {
        Project {
            id: ProjectId("p".into()),
            name: "orbit".into(),
            repo_path: path.to_path_buf(),
            sort_order: 0,
        }
    }

    pub(crate) fn agent(id: &str, worktree: &str, name: &str) -> Agent {
        Agent {
            id: AgentId(id.into()),
            worktree_id: WorktreeId(worktree.into()),
            name: name.into(),
            status: AgentStatus::Running,
            archived: false,
            archived_at: 0,
            unseen: false,
            status_changed_at: 0,
            kind: AgentKind::Claude,
            custom_harness: None,
            model: Some("opus".into()),
            effort: Some("high".into()),
            session_id: Some("s1".into()),
            cloud_session_id: None,
            issue_url: None,
            sort_order: 0,
            alive: true,
            role: AgentRole::Worker,
            recent_prompts: Vec::new(),
        }
    }

    /// One project: the root checkout with a session and an archived one,
    /// and an empty linked checkout.
    pub(crate) fn sample() -> Tree {
        let mut archived = agent("a2", "main", "Old Work");
        archived.archived = true;
        Tree {
            projects: vec![project(Path::new("/code/orbit"))],
            worktrees: vec![
                worktree("main", Path::new("/code/orbit"), true),
                worktree("feat", Path::new("/code/orbit-worktrees/feat"), false),
            ],
            agents: vec![agent("a1", "main", "Fix Login"), archived],
            ..Tree::default()
        }
    }

    // The mirror follows the daemon's deltas: an upsert lands by id, a
    // status change restamps the row, and a removal takes what sat under it.
    #[test]
    fn fold_keeps_the_tree_current() {
        let mut tree = sample();
        let mut renamed = agent("a1", "main", "Renamed");
        renamed.status = AgentStatus::Finished;
        tree.apply(&ServerEvent::EntityUpserted {
            entity: Entity::Agent(renamed),
        });
        assert_eq!(tree.agents.len(), 2, "an upsert of a known id replaces it");
        assert_eq!(tree.agents[0].name, "Renamed");
        tree.apply(&ServerEvent::StatusChanged {
            agent: AgentId("a1".into()),
            status: AgentStatus::NeedsFeedback,
            changed_at: 7,
            unseen: false,
        });
        assert_eq!(tree.agents[0].status, AgentStatus::NeedsFeedback);
        assert_eq!(tree.agents[0].status_changed_at, 7);
        tree.apply(&ServerEvent::EntityRemoved {
            id: EntityId::Project(ProjectId("p".into())),
        });
        assert!(
            tree.worktrees.is_empty() && tree.agents.is_empty(),
            "a removed project takes its rows with it: {tree:?}"
        );
    }

    /// What the TUIs show lands as the daemon says it, and a Snapshot,
    /// which never carries it, leaves it as it stood.
    #[test]
    fn fold_keeps_what_the_tuis_show() {
        let mut tree = sample();
        assert!(!tree.tui_open && tree.tui_session.is_none());
        tree.apply(&ServerEvent::TuiShowing {
            session: Some(AgentId("a1".into())),
            tui_open: true,
        });
        assert!(tree.tui_open);
        assert_eq!(tree.tui_session, Some(AgentId("a1".into())));
        tree.apply(&ServerEvent::Snapshot {
            projects: Vec::new(),
            worktrees: Vec::new(),
            agents: Vec::new(),
            terminals: Vec::new(),
            links: Vec::new(),
            pr_seen: Vec::new(),
            ui_state: None,
        });
        assert!(tree.tui_open);
        assert_eq!(tree.tui_session, Some(AgentId("a1".into())));
        tree.apply(&ServerEvent::TuiShowing {
            session: None,
            tui_open: false,
        });
        assert!(!tree.tui_open && tree.tui_session.is_none());
    }

    fn terminal(id: &str, worktree: &str) -> TerminalTab {
        TerminalTab {
            id: TerminalId(id.into()),
            worktree_id: WorktreeId(worktree.into()),
            name: id.into(),
            sort_order: 0,
            alive: true,
            run_command: None,
        }
    }

    fn link(id: &str, worktree: &str) -> Link {
        Link {
            id: LinkId(id.into()),
            worktree_id: WorktreeId(worktree.into()),
            url: "https://example.com".into(),
            sort_order: 0,
        }
    }

    /// Terminals and links are folded like every other row: upserted by
    /// id, removed by id, and gone with the checkout they are in; a
    /// checkout's deletion takes its terminals and keeps its links, as the
    /// TUI's mirror does.
    #[test]
    fn fold_keeps_terminals_and_links_too() {
        let mut tree = sample();
        for entity in [
            Entity::Terminal(terminal("t1", "main")),
            Entity::Terminal(terminal("t2", "feat")),
            Entity::Link(link("l1", "main")),
            Entity::Link(link("l2", "feat")),
        ] {
            tree.apply(&ServerEvent::EntityUpserted { entity });
        }
        let mut renamed = terminal("t1", "main");
        renamed.name = "renamed".into();
        tree.apply(&ServerEvent::EntityUpserted {
            entity: Entity::Terminal(renamed),
        });
        assert_eq!(tree.terminals.len(), 2);
        assert_eq!(tree.terminals[0].name, "renamed");

        let mut deleted = worktree("feat", Path::new("/code/orbit-worktrees/feat"), false);
        deleted.removed = true;
        tree.apply(&ServerEvent::EntityUpserted {
            entity: Entity::Worktree(deleted),
        });
        let ids = |tree: &Tree| {
            (
                tree.terminals
                    .iter()
                    .map(|t| t.id.0.clone())
                    .collect::<Vec<_>>(),
                tree.links
                    .iter()
                    .map(|l| l.id.0.clone())
                    .collect::<Vec<_>>(),
            )
        };
        assert_eq!(
            ids(&tree),
            (vec!["t1".into()], vec!["l1".into(), "l2".into()])
        );

        tree.apply(&ServerEvent::EntityRemoved {
            id: EntityId::Link(LinkId("l1".into())),
        });
        tree.apply(&ServerEvent::EntityRemoved {
            id: EntityId::Worktree(WorktreeId("feat".into())),
        });
        assert_eq!(ids(&tree), (vec!["t1".into()], vec![]));
        tree.apply(&ServerEvent::EntityRemoved {
            id: EntityId::Worktree(WorktreeId("main".into())),
        });
        assert_eq!(ids(&tree), (vec![], vec![]));
    }

    /// A project upsert lands in `sort_order`, so a reorder moves it.
    #[test]
    fn a_project_upsert_keeps_the_projects_in_order() {
        let mut tree = sample();
        let mut later = project(Path::new("/code/later"));
        later.id = ProjectId("q".into());
        later.sort_order = 2;
        tree.apply(&ServerEvent::EntityUpserted {
            entity: Entity::Project(later.clone()),
        });
        later.sort_order = -1;
        tree.apply(&ServerEvent::EntityUpserted {
            entity: Entity::Project(later),
        });
        let order: Vec<&str> = tree.projects.iter().map(|p| p.id.0.as_str()).collect();
        assert_eq!(order, ["q", "p"]);
    }

    /// The attention order: NEEDS FEEDBACK, RUNNING, UNSEEN, then by last
    /// interaction, never-run last, ties in tree order; archived sessions,
    /// the ORCHESTRATOR HOME's and (on the board) the orchestrator left out.
    #[test]
    fn the_sessions_come_in_attention_order() {
        let mut tree = sample();
        tree.agents.clear();
        let mut add = |id: &str, wt: &str, status, unseen, stamp| {
            let mut a = agent(id, wt, id);
            a.status = status;
            a.unseen = unseen;
            a.status_changed_at = stamp;
            tree.agents.push(a);
        };
        add("fresh", "main", AgentStatus::Fresh, false, 0);
        add("fresh2", "feat", AgentStatus::Fresh, false, 0);
        add("ask", "feat", AgentStatus::NeedsFeedback, false, 10);
        add("run", "main", AgentStatus::Running, false, 20);
        add("run_new", "feat", AgentStatus::Running, false, 30);
        add("read", "feat", AgentStatus::Finished, false, 1_000);
        add("unread", "main", AgentStatus::Finished, true, 500);
        add("old", "main", AgentStatus::Finished, false, 100);
        add("gone", "main", AgentStatus::NeedsFeedback, false, 9_000);
        tree.agents.last_mut().unwrap().archived = true;
        let home = orchestrator_home();
        tree.projects.push(Project {
            id: ProjectId("home".into()),
            name: "orchestrator".into(),
            repo_path: home.to_path_buf(),
            sort_order: 0,
        });
        tree.worktrees.push(Worktree {
            id: WorktreeId("home".into()),
            project_id: ProjectId("home".into()),
            ..worktree("home", home, true)
        });
        let mut orchestrator = agent("orch", "home", "orchestrator");
        orchestrator.role = AgentRole::Orchestrator;
        tree.agents.push(orchestrator);
        let mut second = agent("second", "main", "second");
        second.role = AgentRole::Orchestrator;
        second.status = AgentStatus::Fresh;
        tree.agents.push(second);

        let ids = |ids: Vec<AgentId>| ids.into_iter().map(|id| id.0).collect::<Vec<_>>();
        // Running sessions all count as now, so tree order breaks their tie.
        let order = [
            "ask", "run", "run_new", "unread", "read", "old", "fresh", "second", "fresh2",
        ];
        assert_eq!(ids(tree.attention_sessions()), order);
        let orchestrator = |tree: &Tree| tree.orchestrator_agent().map(|a| a.id.0.clone());
        assert_eq!(orchestrator(&tree).as_deref(), Some("orch"));
        assert_eq!(
            ids(tree.board_sessions()),
            order,
            "the orchestrator in its home is in neither"
        );
        tree.agents.retain(|a| a.id.0 != "orch");
        assert_eq!(
            orchestrator(&tree).as_deref(),
            Some("second"),
            "the first live row with the role"
        );
        let without: Vec<&str> = order.into_iter().filter(|id| *id != "second").collect();
        assert_eq!(ids(tree.board_sessions()), without);
    }
}
