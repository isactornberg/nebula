//! MERGE NOTES: once this client sees a checkout's pull request merge
//! (the moment [`App::note_merge_landed`] stamps, the signal the
//! `auto_cleanup_merged` setting acts on), and the checkout's project has a
//! CAPTAIN, the captain is told in one note typed in as its next turn:
//! `[nebula] PR #42 "Fix the login loop" merged from fix-login. Sessions
//! that worked on it: 01K…, 01K….` Its guidance has it read those
//! sessions' final replies and keep what a future agent in the project
//! should know.
//!
//! This client decides which notes go, and hands each to the DAEMON
//! (`ClientRequest::MergeNote`), which types it as it types every turn
//! nobody at a session's keys typed: once the captain is at rest with its
//! input box empty and asks the user nothing, one turn at a time. A note is
//! handed over once per pull request, ever: every note queued, handed over
//! or not, is kept in `<data dir>/merge-notes.json` and read again before
//! each change, so neither a restart nor a second client on the same daemon
//! hands one over twice, and a note still held when the TUI quits goes from
//! the next one. A daemon older than [`MERGE_NOTE_PROTOCOL`] is handed
//! none: typed beside its own turns, a note could share an input box with
//! one, so the notes wait in the file for a daemon that is new.

use crate::app::App;
use nebula_core::{
    Agent, AgentId, AgentRole, ClientRequest, ProjectId, Worktree, WorktreeId, MERGE_NOTE_PROTOCOL,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// The file under the DATA DIR.
const FILE: &str = "merge-notes.json";
/// How many notes the file keeps: the oldest sent ones go first, and a
/// held one only once there are no sent ones left to drop.
const KEEP: usize = 100;
/// What the status line says, once a run, while a note waits for a daemon
/// new enough to type it.
pub(crate) const DAEMON_TOO_OLD: &str =
    "merge note held: the daemon is older than this nebula, restart it first (nebula reload)";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeNote {
    /// The pull request's URL: what a note is sent once for.
    pub url: String,
    pub project: ProjectId,
    pub text: String,
    /// Handed to the daemon, which types it.
    pub sent: bool,
}

/// The notes this client knows of, and the file they live in.
#[derive(Debug, Default)]
pub struct MergeNotes {
    /// The main loop installs the real file at startup; the unit tests
    /// leave it `None`, so no test touches the user's.
    path: Option<PathBuf>,
    pub notes: Vec<MergeNote>,
    /// The status line has said a note waits for a newer daemon.
    said_daemon_old: bool,
}

impl MergeNotes {
    /// The notes in `path`, which saves keep up to date.
    pub fn at(path: PathBuf) -> Self {
        let mut notes = Self {
            path: Some(path),
            ..Self::default()
        };
        notes.reload();
        notes
    }

    /// The real one: `<data dir>/merge-notes.json`.
    pub fn default_location() -> Self {
        Self::at(nebula_core::paths::data_dir().join(FILE))
    }

    /// What the file says now, which another client may have changed. A
    /// missing or unreadable file leaves what this one knows.
    fn reload(&mut self) {
        let Some(path) = &self.path else { return };
        let read = std::fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok());
        if let Some(notes) = read {
            self.notes = notes;
        }
    }

    fn save(&mut self) {
        let mut excess = self.notes.len().saturating_sub(KEEP);
        self.notes.retain(|n| {
            let drop = excess > 0 && n.sent;
            excess -= usize::from(drop);
            !drop
        });
        self.notes.drain(..excess);
        let Some(path) = &self.path else { return };
        if let Err(err) = crate::pr_cache::write_json_atomic(path, &self.notes) {
            tracing::warn!("merge notes not written: {err}");
        }
    }
}

/// The project's CAPTAIN, if it has one.
fn captain_of<'a>(app: &'a App, project: &ProjectId) -> Option<&'a Agent> {
    nebula_core::live_captain(app.tree.agents.iter().filter(|a| {
        app.tree
            .worktrees
            .iter()
            .any(|w| w.id == a.worktree_id && &w.project_id == project)
    }))
}

/// The checkouts of every project with a CAPTAIN that a live session of
/// its works in: where a merge is a note to send. The PR sweep covers
/// them whichever project is on screen, on every GIT POLL, so this is one
/// pass over the checkouts and one over the sessions.
pub fn led_checkouts(app: &App) -> Vec<&Worktree> {
    let project_of: HashMap<&WorktreeId, &ProjectId> = app
        .tree
        .worktrees
        .iter()
        .map(|w| (&w.id, &w.project_id))
        .collect();
    let mut led: HashSet<&ProjectId> = HashSet::new();
    let mut worked: HashSet<&WorktreeId> = HashSet::new();
    for a in app.tree.agents.iter().filter(|a| !a.archived) {
        match a.role {
            AgentRole::Captain => led.extend(project_of.get(&a.worktree_id)),
            AgentRole::Worker => {
                worked.insert(&a.worktree_id);
            }
            _ => {}
        }
    }
    app.tree
        .worktrees
        .iter()
        .filter(|w| worked.contains(&w.id) && led.contains(&w.project_id))
        .collect()
}

/// Session `id`'s turn just finished: a WORKER's, in a checkout a CAPTAIN
/// leads, brings the checkout's next pull-request lookup forward to now,
/// since a turn is what opens a pull request, and so the captain hears of
/// its merge sooner. The backoff counts on from that lookup, so it costs
/// no `gh` call beyond the one it moves.
pub fn worker_finished(app: &mut App, id: &AgentId) {
    let Some(worktree) = app
        .tree
        .agents
        .iter()
        .find(|a| &a.id == id && a.role == AgentRole::Worker)
        .map(|a| a.worktree_id.clone())
    else {
        return;
    };
    if !led_checkouts(app).iter().any(|w| w.id == worktree) {
        return;
    }
    if let Some((due, _)) = app.github.pr_recheck.get_mut(&worktree) {
        *due = std::time::Instant::now();
    }
}

/// `worktree`'s pull request was merged when this client first looked: it
/// merged while no client was looking, or between two looks. A checkout a
/// live worker of a project with a CAPTAIN is still in gets its note as a
/// merge seen to happen does, once per pull request as every note is.
pub fn first_seen_merged(app: &mut App, worktree: &WorktreeId) {
    if led_checkouts(app).iter().any(|w| &w.id == worktree) {
        landed(app, worktree);
    }
}

/// The note for a pull request merged from `branch`, naming the sessions
/// that worked on it.
fn note_text(number: u64, title: &str, branch: &str, sessions: &[&str]) -> String {
    let mut text = format!("[nebula] PR #{number} \"{title}\" merged from {branch}.");
    if !sessions.is_empty() {
        text.push_str(&format!(
            " Sessions that worked on it: {}.",
            sessions.join(", ")
        ));
    }
    text
}

/// `worktree`'s pull request was just seen to merge: queue the note for
/// its project's CAPTAIN, unless the project has none or this pull request
/// has had one already. The sessions it names are the checkout's workers,
/// archived ones too, taken now: a cleanup of the checkout takes their
/// rows with it.
pub fn landed(app: &mut App, worktree: &WorktreeId) {
    let Some(checkout) = app.tree.worktrees.iter().find(|w| &w.id == worktree) else {
        return;
    };
    let Some(Some(pr)) = app.github.pull_requests.get(worktree) else {
        return;
    };
    if captain_of(app, &checkout.project_id).is_none() {
        return;
    }
    let sessions: Vec<&str> = app
        .tree
        .agents
        .iter()
        .filter(|a| a.worktree_id == *worktree && a.role == AgentRole::Worker)
        .map(|a| a.id.as_str())
        .collect();
    let text = note_text(pr.number, &pr.title, &checkout.branch, &sessions);
    let note = MergeNote {
        url: pr.url.clone(),
        project: checkout.project_id.clone(),
        text,
        sent: false,
    };
    let notes = &mut app.github.merge_notes;
    notes.reload();
    if notes.notes.iter().any(|n| n.url == note.url) {
        return;
    }
    notes.notes.push(note);
    notes.save();
}

/// Hand each held note whose project has a CAPTAIN to the DAEMON, which
/// types it into the captain once it is free for it: from here on the note
/// counts as sent, here and in every other client. A daemon older than
/// [`MERGE_NOTE_PROTOCOL`] is handed none, and the status line says so once.
/// Runs on the GIT POLL.
pub fn deliver(app: &mut App, out: &mut Vec<ClientRequest>) {
    if app.github.merge_notes.notes.iter().all(|n| n.sent) {
        return;
    }
    let mut led: HashSet<ProjectId> = HashSet::new();
    for note in app.github.merge_notes.notes.iter().filter(|n| !n.sent) {
        if !led.contains(&note.project) && captain_of(app, &note.project).is_some() {
            led.insert(note.project.clone());
        }
    }
    let due = |n: &MergeNote| !n.sent && led.contains(&n.project);
    let notes = &mut app.github.merge_notes;
    if !notes.notes.iter().any(due) {
        return;
    }
    if app.requests.daemon_protocol < MERGE_NOTE_PROTOCOL {
        if !std::mem::replace(&mut notes.said_daemon_old, true) {
            app.chrome.flash = Some(DAEMON_TOO_OLD.into());
            app.chrome.dirty = true;
        }
        return;
    }
    notes.reload();
    let handed: Vec<MergeNote> = notes
        .notes
        .iter_mut()
        .filter(|n| due(n))
        .map(|n| {
            n.sent = true;
            n.clone()
        })
        .collect();
    if handed.is_empty() {
        return;
    }
    notes.save();
    for note in handed {
        let req_id = app.alloc_req_id(crate::app::PendingIntent::None);
        out.push(ClientRequest::MergeNote {
            req_id,
            project: note.project,
            pr_url: note.url,
            text: note.text,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::{AgentKind, AgentStatus, Project};
    use nebula_forge::pull_request::{PullRequest, STATE_MERGED, STATE_OPEN};
    use std::time::Duration;

    fn agent(id: &str, worktree: &str, role: AgentRole) -> Agent {
        Agent {
            id: AgentId(id.into()),
            worktree_id: WorktreeId(worktree.into()),
            name: id.into(),
            status: AgentStatus::Finished,
            archived: false,
            archived_at: 0,
            unseen: false,
            status_changed_at: 1,
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            issue_url: None,
            role,
            sort_order: 0,
            alive: true,
            recent_prompts: Vec::new(),
        }
    }

    fn pr(state: &str) -> PullRequest {
        PullRequest {
            number: 42,
            url: "https://github.com/o/r/pull/42".into(),
            title: "Fix the login loop".into(),
            state: state.into(),
            is_draft: false,
            head: "fix-login".into(),
            head_sha: "abc".into(),
            base: "main".into(),
            health: Default::default(),
            activity: Vec::new(),
        }
    }

    /// A project with a captain at rest in its root and two workers on
    /// `fix-login`, one of them archived, and that checkout's open PR.
    fn app() -> App {
        let mut app = App::new();
        app.tree.projects.push(Project {
            id: ProjectId("p".into()),
            name: "api".into(),
            repo_path: "/tmp/api".into(),
            sort_order: 0,
        });
        for (id, branch, is_main) in [("root", "main", true), ("w", "fix-login", false)] {
            app.tree.worktrees.push(Worktree {
                id: WorktreeId(id.into()),
                project_id: ProjectId("p".into()),
                path: format!("/tmp/api/{id}").into(),
                branch: branch.into(),
                is_main,
                sort_order: 0,
                removed: false,
            });
        }
        app.tree
            .agents
            .push(agent("captain", "root", AgentRole::Captain));
        app.tree.agents.push(agent("a1", "w", AgentRole::Worker));
        let mut archived = agent("a2", "w", AgentRole::Worker);
        archived.archived = true;
        app.tree.agents.push(archived);
        app.github
            .pull_requests
            .insert(WorktreeId("w".into()), Some(pr(STATE_OPEN)));
        app
    }

    fn merge(app: &mut App) {
        crate::event_loop::land_pull_request(
            app,
            WorktreeId("w".into()),
            nebula_forge::pull_request::Lookup::Found(pr(STATE_MERGED)),
        );
    }

    /// The notes [`super::deliver`] handed the daemon on this poll, as
    /// `(project, pull request, text)`.
    fn deliver(app: &mut App) -> Vec<(String, String, String)> {
        let mut out = Vec::new();
        super::deliver(app, &mut out);
        out.into_iter()
            .map(|r| match r {
                ClientRequest::MergeNote {
                    project,
                    pr_url,
                    text,
                    ..
                } => (project.0, pr_url, text),
                other => panic!("only merge notes are sent: {other:?}"),
            })
            .collect()
    }

    const NOTE: &str = "[nebula] PR #42 \"Fix the login loop\" merged from fix-login. \
                        Sessions that worked on it: a1, a2.";

    fn handed(text: &str) -> Vec<(String, String, String)> {
        vec![(
            "p".into(),
            "https://github.com/o/r/pull/42".into(),
            text.into(),
        )]
    }

    /// A merge seen to happen hands the daemon one note for the project's
    /// captain, naming the checkout's workers, archived ones included,
    /// whatever the captain is doing: the daemon holds it until the captain
    /// is free. The merged row answering again hands over nothing more.
    #[test]
    fn a_merge_tells_the_captain_once() {
        let mut app = app();
        app.tree.agents[0].status = AgentStatus::Running;
        merge(&mut app);
        assert_eq!(deliver(&mut app), handed(NOTE));
        assert!(app.github.merge_notes.notes[0].sent);
        merge(&mut app);
        assert!(deliver(&mut app).is_empty());
    }

    /// A daemon older than the gate types the notes no TUI may type beside
    /// it: it is handed none, the status line says why once, and the note
    /// waits, held, for a daemon that is new.
    #[test]
    fn an_older_daemon_is_handed_no_note() {
        let mut app = app();
        app.requests.daemon_protocol = MERGE_NOTE_PROTOCOL - 1;
        merge(&mut app);
        assert!(deliver(&mut app).is_empty());
        assert_eq!(app.chrome.flash.as_deref(), Some(DAEMON_TOO_OLD));
        app.chrome.flash = None;
        assert!(deliver(&mut app).is_empty());
        assert_eq!(app.chrome.flash, None, "said once");
        assert!(!app.github.merge_notes.notes[0].sent);
        app.requests.daemon_protocol = MERGE_NOTE_PROTOCOL;
        assert_eq!(deliver(&mut app), handed(NOTE));
    }

    /// The sweep asks for the led checkouts on every GIT POLL: a machine
    /// with a thousand checkouts and twice the sessions answers at once.
    #[test]
    fn led_checkouts_is_one_pass() {
        let mut app = app();
        for i in 0..1000 {
            app.tree.worktrees.push(Worktree {
                id: WorktreeId(format!("w{i}")),
                project_id: ProjectId("p".into()),
                path: format!("/tmp/api/w{i}").into(),
                branch: format!("b{i}"),
                is_main: false,
                sort_order: 0,
                removed: false,
            });
            for n in 0..2 {
                let id = format!("a{i}-{n}");
                app.tree
                    .agents
                    .push(agent(&id, &format!("w{i}"), AgentRole::Worker));
            }
        }
        let began = std::time::Instant::now();
        assert_eq!(led_checkouts(&app).len(), 1001);
        assert!(
            began.elapsed() < Duration::from_secs(1),
            "{:?}",
            began.elapsed()
        );
    }

    /// A worker's finished turn brings its led checkout's next lookup
    /// forward, keeping the backoff's step; a captain's does not.
    #[test]
    fn a_workers_finish_brings_its_checkouts_lookup_forward() {
        let mut app = app();
        let w = WorktreeId("w".into());
        let later = std::time::Instant::now() + Duration::from_secs(180);
        let step = Duration::from_secs(180);
        app.github.pr_recheck.insert(w.clone(), (later, step));
        worker_finished(&mut app, &AgentId("captain".into()));
        assert!(!app.pr_lookup_due(&w));
        worker_finished(&mut app, &AgentId("a1".into()));
        assert!(app.pr_lookup_due(&w));
        assert_eq!(app.github.pr_recheck[&w].1, step);
    }

    /// A pull request already merged when the checkout is first looked
    /// at is noted too, while a live worker is still in the checkout, and
    /// only once.
    #[test]
    fn a_merge_first_seen_merged_is_noted() {
        let mut app = app();
        app.github.pull_requests.clear();
        merge(&mut app);
        assert_eq!(deliver(&mut app).len(), 1);

        let mut gone = self::app();
        gone.github.pull_requests.clear();
        gone.tree.agents[1].archived = true;
        merge(&mut gone);
        assert!(
            gone.github.merge_notes.notes.is_empty(),
            "no worker left in it"
        );
    }

    /// A project with no captain gets no note; one held for a project whose
    /// captain went waits, held, for the next.
    #[test]
    fn no_captain_no_note() {
        let mut app = app();
        app.tree.agents[0].archived = true;
        merge(&mut app);
        assert!(app.github.merge_notes.notes.is_empty());

        let mut app = self::app();
        app.tree.agents[0].status = AgentStatus::Running;
        merge(&mut app);
        app.tree.agents[0].archived = true;
        assert!(deliver(&mut app).is_empty());
        app.tree
            .agents
            .push(agent("captain2", "root", AgentRole::Captain));
        assert_eq!(deliver(&mut app), handed(NOTE));
    }

    /// With two live captains (a retired one unarchived), the board goes by
    /// the one the daemon's `captain` returns, and hands merge notes to: the
    /// most recently created, whichever comes first in the tree.
    #[test]
    fn two_live_captains_read_as_the_newer_one() {
        let mut app = app();
        // ULIDs sort by creation: `captain` is the newer.
        app.tree
            .agents
            .push(agent("captai0", "root", AgentRole::Captain));
        app.tree.agents.reverse();
        let board = crate::board::board(&app);
        let captains: Vec<&str> = board
            .rows
            .iter()
            .filter(|r| matches!(r.kind, crate::board::RowKind::Captain { .. }))
            .map(|r| r.id.0.as_str())
            .collect();
        assert_eq!(captains, ["captain"], "{board:#?}");
    }

    /// The file is what a restart, or a second client, goes by: a note
    /// handed over is never handed over again, and one held when the TUI
    /// quit goes from the next.
    #[test]
    fn the_file_outlasts_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let mut app = app();
        app.github.merge_notes = MergeNotes::at(path.clone());
        app.requests.daemon_protocol = MERGE_NOTE_PROTOCOL - 1;
        merge(&mut app);

        let mut next = app_with_cache(&path);
        next.github
            .pull_requests
            .insert(WorktreeId("w".into()), Some(pr(STATE_MERGED)));
        assert_eq!(deliver(&mut next), handed(NOTE), "the held note goes");

        let mut third = app_with_cache(&path);
        merge(&mut third);
        assert!(deliver(&mut third).is_empty(), "handed over once, ever");
        app.requests.daemon_protocol = MERGE_NOTE_PROTOCOL;
        assert!(
            deliver(&mut app).is_empty(),
            "the first client sees it sent"
        );
    }

    fn app_with_cache(path: &std::path::Path) -> App {
        let mut app = app();
        app.github.merge_notes = MergeNotes::at(path.to_path_buf());
        app
    }

    /// The PR sweep reaches a captain's project whichever is on screen:
    /// the checkouts its live workers are in, never its root.
    #[test]
    fn the_sweep_covers_a_captains_checkouts() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app();
        app.tree.worktrees[1].path = dir.path().to_path_buf();
        let ids = |app: &App| {
            led_checkouts(app)
                .iter()
                .map(|w| w.id.0.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&app), ["w"]);
        assert_eq!(
            crate::event_loop::sweep_target(&mut app).map(|(id, _)| id),
            Some(WorktreeId("w".into()))
        );
        app.tree.agents[1].archived = true;
        assert!(ids(&app).is_empty(), "only a live worker's checkout");
    }

    /// Over its cap the file drops its oldest sent notes, and a held one
    /// only once no sent one is left to drop.
    #[test]
    fn the_file_drops_sent_notes_first() {
        let note = |i: usize, sent: bool| MergeNote {
            url: i.to_string(),
            project: ProjectId("p".into()),
            text: String::new(),
            sent,
        };
        // The oldest held, then sent and held turn about.
        let mut notes = MergeNotes {
            notes: (0..KEEP + 5)
                .map(|i| note(i, i > 0 && i % 2 == 0))
                .collect(),
            ..MergeNotes::default()
        };
        notes.save();
        assert_eq!(notes.notes.len(), KEEP);
        let urls: Vec<&str> = notes.notes.iter().map(|n| n.url.as_str()).collect();
        assert_eq!(urls[..5], ["0", "1", "3", "5", "7"], "{urls:?}");
        assert!(notes.notes.iter().filter(|n| !n.sent).count() == KEEP / 2 + 3);

        let mut held = MergeNotes {
            notes: (0..KEEP + 5).map(|i| note(i, false)).collect(),
            ..MergeNotes::default()
        };
        held.save();
        assert_eq!(held.notes.len(), KEEP);
        assert_eq!(
            held.notes[0].url, "5",
            "the oldest held go once none is sent"
        );
    }

    /// Two clients on one daemon share what each handed over: the second
    /// hands over none the first did, and its own at once, for the daemon
    /// to type once the captain has taken the first.
    #[test]
    fn a_second_client_hands_over_only_what_the_first_did_not() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let mut first = app_with_cache(&path);
        merge(&mut first);
        assert_eq!(deliver(&mut first).len(), 1);

        let mut second = app_with_cache(&path);
        second.github.merge_notes.notes.push(MergeNote {
            url: "https://github.com/o/r/pull/43".into(),
            project: ProjectId("p".into()),
            text: "second".into(),
            sent: false,
        });
        second.github.merge_notes.save();
        assert_eq!(
            deliver(&mut second),
            [(
                "p".to_string(),
                "https://github.com/o/r/pull/43".to_string(),
                "second".to_string()
            )]
        );
        assert!(deliver(&mut first).is_empty());
    }
}
