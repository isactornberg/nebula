//! MERGE NOTES: once this client sees a checkout's pull request merge
//! (the moment [`App::note_merge_landed`] stamps, the signal the
//! `auto_cleanup_merged` setting acts on), and the checkout's project has a
//! CAPTAIN, the captain is told in one note typed in as its next turn:
//! `[nebula] PR #42 "Fix the login loop" merged from fix-login. Sessions
//! that worked on it: 01K…, 01K….` Its guidance has it read those
//! sessions' final replies and keep what a future agent in the project
//! should know.
//!
//! A note waits for the captain to be up and at rest: live, with its last
//! turn finished. It is sent once per pull request, ever, and a captain is
//! sent the next only once it has taken the last: every note queued, sent
//! or not, is kept in `<data dir>/merge-notes.json` with the captain and
//! the turn it went to, and read again before each change, so neither a
//! restart nor a second client on the same daemon sends one twice, and a
//! note still held when the TUI quits goes out from the next one.

use crate::app::App;
use nebula_core::{
    Agent, AgentId, AgentRole, AgentStatus, ClientRequest, ProjectId, SessionRef, Worktree,
    WorktreeId,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

/// The file under the DATA DIR.
const FILE: &str = "merge-notes.json";
/// How many notes the file keeps: the oldest sent ones go first, and a
/// held one only once there are no sent ones left to drop.
const KEEP: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeNote {
    /// The pull request's URL: what a note is sent once for.
    pub url: String,
    pub project: ProjectId,
    pub text: String,
    pub sent: bool,
    /// The captain it was typed into, and that captain's status stamp
    /// then: it is sent the next note only once that has moved on, since
    /// the word that it took this one lags the keys.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent_to: Option<(AgentId, i64)>,
}

/// The notes this client knows of, and the file they live in.
#[derive(Debug, Default)]
pub struct MergeNotes {
    /// The main loop installs the real file at startup; the unit tests
    /// leave it `None`, so no test touches the user's.
    path: Option<PathBuf>,
    pub notes: Vec<MergeNote>,
    /// The CAPTAINS whose screens were asked for, and when, a note waiting
    /// on the answer ([`land_read`]).
    reading: HashMap<AgentId, std::time::Instant>,
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
        sent_to: None,
    };
    let notes = &mut app.github.merge_notes;
    notes.reload();
    if notes.notes.iter().any(|n| n.url == note.url) {
        return;
    }
    notes.notes.push(note);
    notes.save();
}

/// How long a captain must have had no key from this client for a note
/// to be typed into it.
const TYPING_QUIET: Duration = Duration::from_secs(5);
/// How long a read of a captain's screen is waited on before it is asked
/// again.
const READ_WAIT: Duration = Duration::from_secs(10);

/// Is `captain` where a note can go: live, its last turn finished, and
/// left alone by the user here for [`TYPING_QUIET`]?
fn at_rest(app: &App, captain: &Agent) -> bool {
    let typing = app
        .pane
        .typed_at
        .get(&SessionRef::Agent(captain.id.clone()))
        .is_some_and(|at| at.elapsed() < TYPING_QUIET);
    captain.alive && captain.status == AgentStatus::Finished && !typing
}

/// The project whose CAPTAIN `id` is, while it is.
fn led_by(app: &App, id: &AgentId) -> Option<ProjectId> {
    let a = app.tree.agents.iter().find(|a| &a.id == id)?;
    let project = app
        .tree
        .worktrees
        .iter()
        .find(|w| w.id == a.worktree_id)?
        .project_id
        .clone();
    (captain_of(app, &project).map(|c| &c.id) == Some(id)).then_some(project)
}

/// Read the screen of each CAPTAIN with a note held for it that is at
/// rest ([`at_rest`]) and has moved on since the last note it was sent,
/// by this client or another: the note goes once the read shows its input
/// box empty ([`land_read`]). One note per captain at a time; the next
/// waits for it to take this one and come to rest again. Runs on the GIT
/// POLL.
pub fn deliver(app: &mut App, out: &mut Vec<ClientRequest>) {
    if app.github.merge_notes.notes.iter().all(|n| n.sent) {
        return;
    }
    let mut projects: Vec<ProjectId> = Vec::new();
    for note in app.github.merge_notes.notes.iter().filter(|n| !n.sent) {
        if !projects.contains(&note.project) {
            projects.push(note.project.clone());
        }
    }
    for project in projects {
        let Some(captain) = captain_of(app, &project).filter(|c| at_rest(app, c)) else {
            continue;
        };
        let id = captain.id.clone();
        let sent_to = Some((id.clone(), captain.status_changed_at));
        let notes = &mut app.github.merge_notes;
        if notes
            .reading
            .get(&id)
            .is_some_and(|at| at.elapsed() < READ_WAIT)
        {
            continue;
        }
        notes.reload();
        if notes.notes.iter().any(|n| n.sent_to == sent_to) {
            continue;
        }
        notes.reading.insert(id.clone(), std::time::Instant::now());
        let req_id = app.alloc_req_id(crate::app::PendingIntent::MergeNoteRead);
        out.push(ClientRequest::TailOutput {
            req_id,
            session: SessionRef::Agent(id),
            max_bytes: crate::terminal_tail::SCREEN_TAIL_BYTES,
            after_seq: None,
        });
    }
}

/// The answer to [`deliver`]'s read of CAPTAIN `id`'s screen: when its
/// input box is empty, and it is still at rest, its project's oldest held
/// note is typed into it as its next turn. A box with anything the user
/// typed in it, or a screen with no box, leaves the note for a later poll.
pub fn land_read(
    app: &mut App,
    id: AgentId,
    tail: Option<nebula_core::OutputTail>,
    out: &mut Vec<ClientRequest>,
) {
    app.github.merge_notes.reading.remove(&id);
    let Some(project) = led_by(app, &id) else {
        return;
    };
    let Some(captain) = captain_of(app, &project).filter(|c| at_rest(app, c)) else {
        return;
    };
    if !tail.as_ref().is_some_and(box_is_empty) {
        return;
    }
    let sent_to = Some((id.clone(), captain.status_changed_at));
    let notes = &mut app.github.merge_notes;
    notes.reload();
    if notes.notes.iter().any(|n| n.sent_to == sent_to) {
        return;
    }
    let Some(note) = notes
        .notes
        .iter_mut()
        .find(|n| n.project == project && !n.sent)
    else {
        return;
    };
    note.sent = true;
    note.sent_to = sent_to;
    let text = note.text.clone();
    notes.save();
    let session = SessionRef::Agent(id);
    out.push(ClientRequest::Input {
        session: session.clone(),
        data: crate::event_loop::turn_bytes(&text),
    });
    crate::event_loop::hold_enter(app, session);
    let name = app
        .tree
        .projects
        .iter()
        .find(|p| p.id == project)
        .map_or("the project", |p| p.name.as_str());
    app.chrome.flash = Some(format!("told {name}'s captain a pull request merged"));
    app.chrome.dirty = true;
}

/// Does the input box on the screen in `tail` hold nothing the user typed?
/// Its row is the last one that opens on a prompt mark, past any frame,
/// and everything after the mark must be blank or drawn dim: Claude draws
/// the suggestion it offers in an empty box dim. A screen with no such row
/// has no box to type into.
fn box_is_empty(tail: &nebula_core::OutputTail) -> bool {
    let mut parser = vt100::Parser::new(tail.rows.max(2), tail.cols.max(2), 0);
    parser.process(&tail.data);
    let screen = parser.screen();
    let (rows, cols) = screen.size();
    let blank = |c: &vt100::Cell| {
        c.contents()
            .trim_matches(|ch: char| ch.is_whitespace() || ch == '│' || ch == '┃')
            .is_empty()
    };
    for row in (0..rows).rev() {
        let Some(at) = (0..cols).find(|&c| screen.cell(row, c).is_some_and(|c| !blank(c))) else {
            continue;
        };
        let mark = screen.cell(row, at).map(vt100::Cell::contents);
        if !matches!(mark, Some("❯" | "›" | ">")) {
            continue;
        }
        return (at + 1..cols).all(|c| screen.cell(row, c).is_none_or(|c| blank(c) || c.dim()));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pull_request::{PullRequest, STATE_MERGED, STATE_OPEN};
    use nebula_core::{AgentKind, Project};

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
            crate::pull_request::Lookup::Found(pr(STATE_MERGED)),
        );
    }

    /// A captain's screen at rest, its input box holding `typed`.
    fn screen(typed: &str) -> nebula_core::OutputTail {
        let rule = "─".repeat(40);
        let data = format!(
            "⏺ Draft PRs sink is on it.\r\n\r\n  DONE started Draft PRs sink\r\n\r\n\
             {rule}\r\n❯\u{a0}{typed}\r\n{rule}\r\n  ⏵⏵ auto mode on\r\n"
        );
        nebula_core::OutputTail {
            cols: 60,
            rows: 12,
            end_seq: data.len() as u64,
            data: data.into_bytes(),
        }
    }

    /// The captains `deliver` asked to read.
    fn reads(out: &[ClientRequest]) -> Vec<AgentId> {
        out.iter()
            .filter_map(|r| match r {
                ClientRequest::TailOutput {
                    session: SessionRef::Agent(id),
                    ..
                } => Some(id.clone()),
                _ => None,
            })
            .collect()
    }

    /// [`super::deliver`], with each read it asks for answered with `box`
    /// in the captain's input box.
    fn deliver_with(app: &mut App, typed: &str, out: &mut Vec<ClientRequest>) {
        let mut asks = Vec::new();
        super::deliver(app, &mut asks);
        for id in reads(&asks) {
            land_read(app, id, Some(screen(typed)), out);
        }
    }

    /// [`deliver_with`] an empty box.
    fn deliver(app: &mut App, out: &mut Vec<ClientRequest>) {
        deliver_with(app, "", out);
    }

    /// A note is typed only into an empty box: one the user has half a
    /// prompt in waits, and one holding no more than the suggestion
    /// Claude draws dim in an empty box takes it.
    #[test]
    fn a_note_waits_for_an_empty_box() {
        let mut app = app();
        merge(&mut app);
        let mut out = Vec::new();
        deliver_with(&mut app, "also check the", &mut out);
        assert!(out.is_empty(), "typed into a half-written prompt: {out:?}");
        assert!(!app.github.merge_notes.notes[0].sent);
        deliver_with(
            &mut app,
            "\x1b[2;7mW\x1b[0;2mait for the report\x1b[0m",
            &mut out,
        );
        assert_eq!(typed(&out).len(), 1, "{out:?}");
        // A screen with no box on it is no box to type into.
        let tail = nebula_core::OutputTail {
            cols: 40,
            rows: 4,
            end_seq: 4,
            data: b"Do you trust the files in this folder?".to_vec(),
        };
        assert!(!box_is_empty(&tail));
    }

    /// A note waits for the user to have stopped typing at the captain
    /// for a few seconds; nothing is even read meanwhile.
    #[test]
    fn a_note_waits_for_the_user_to_stop_typing() {
        let mut app = app();
        merge(&mut app);
        let captain = SessionRef::Agent(AgentId("captain".into()));
        app.pane
            .typed_at
            .insert(captain.clone(), std::time::Instant::now());
        let mut asks = Vec::new();
        super::deliver(&mut app, &mut asks);
        assert!(asks.is_empty(), "{asks:?}");
        let quiet = std::time::Instant::now() - TYPING_QUIET - Duration::from_secs(1);
        app.pane.typed_at.insert(captain, quiet);
        super::deliver(&mut app, &mut asks);
        assert_eq!(reads(&asks), [AgentId("captain".into())]);
        // One read at a time: the next poll waits for its answer.
        let mut again = Vec::new();
        super::deliver(&mut app, &mut again);
        assert!(again.is_empty(), "{again:?}");
    }

    fn typed(out: &[ClientRequest]) -> Vec<String> {
        out.iter()
            .filter_map(|r| match r {
                ClientRequest::Input { session, data } if data != b"\r" => {
                    Some(format!("{session:?}: {}", String::from_utf8_lossy(data)))
                }
                _ => None,
            })
            .collect()
    }

    const NOTE: &str = "[nebula] PR #42 \"Fix the login loop\" merged from fix-login. \
                        Sessions that worked on it: a1, a2.";

    /// A merge seen to happen sends the project's captain one note, naming
    /// the checkout's workers, archived ones included, its Enter held back
    /// to follow on its own; the merged row answering again sends nothing
    /// more.
    #[test]
    fn a_merge_tells_the_captain_once() {
        let mut app = app();
        merge(&mut app);
        let mut out = Vec::new();
        deliver(&mut app, &mut out);
        let captain = SessionRef::Agent(AgentId("captain".into()));
        assert_eq!(typed(&out), [format!("{captain:?}: {NOTE}")]);
        assert_eq!(out.len(), 1, "the Enter is held back: {out:?}");
        let held: Vec<&SessionRef> = app.requests.held_enters.iter().map(|(_, s)| s).collect();
        assert_eq!(held, [&captain]);
        merge(&mut app);
        app.tree.agents[0].status_changed_at = 2;
        let mut out = Vec::new();
        deliver(&mut app, &mut out);
        assert!(out.is_empty(), "{out:?}");
    }

    /// A note waits while the captain is at work, waiting on the user, or
    /// not up, and goes once it is at rest; a second waits for the captain
    /// to take the first.
    #[test]
    fn a_note_waits_for_the_captain_to_be_at_rest() {
        let mut app = app();
        for (status, alive) in [
            (AgentStatus::Running, true),
            (AgentStatus::NeedsFeedback, true),
            (AgentStatus::Disconnected, false),
            (AgentStatus::Finished, false),
        ] {
            app.tree.agents[0].status = status;
            app.tree.agents[0].alive = alive;
            merge(&mut app);
            let mut out = Vec::new();
            deliver(&mut app, &mut out);
            assert!(out.is_empty(), "{status:?} alive={alive}: {out:?}");
        }
        app.tree.agents[0].status = AgentStatus::Finished;
        app.tree.agents[0].alive = true;
        app.github.merge_notes.notes.push(MergeNote {
            url: "https://github.com/o/r/pull/43".into(),
            project: ProjectId("p".into()),
            text: "second".into(),
            sent: false,
            sent_to: None,
        });
        let mut out = Vec::new();
        deliver(&mut app, &mut out);
        assert_eq!(typed(&out).len(), 1, "one at a time: {out:?}");
        let mut out = Vec::new();
        deliver(&mut app, &mut out);
        assert!(out.is_empty(), "the captain has not taken it yet");
        app.tree.agents[0].status_changed_at = 3;
        deliver(&mut app, &mut out);
        assert_eq!(
            typed(&out),
            [format!(
                "{:?}: second",
                SessionRef::Agent(AgentId("captain".into()))
            )]
        );
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
        let mut out = Vec::new();
        deliver(&mut app, &mut out);
        assert_eq!(typed(&out).len(), 1, "{out:?}");

        let mut gone = self::app();
        gone.github.pull_requests.clear();
        gone.tree.agents[1].archived = true;
        merge(&mut gone);
        assert!(
            gone.github.merge_notes.notes.is_empty(),
            "no worker left in it"
        );
    }

    /// A project with no captain gets no note, then or later.
    #[test]
    fn no_captain_no_note() {
        let mut app = app();
        app.tree.agents[0].archived = true;
        merge(&mut app);
        assert!(app.github.merge_notes.notes.is_empty());
    }

    /// With two live captains (a retired one unarchived), the note and the
    /// board both go by the one the daemon's `captain` returns: the most
    /// recently created, whichever comes first in the tree.
    #[test]
    fn two_live_captains_read_as_the_newer_one() {
        let mut app = app();
        // ULIDs sort by creation: `captain` is the newer.
        app.tree
            .agents
            .push(agent("captai0", "root", AgentRole::Captain));
        merge(&mut app);
        let mut out = Vec::new();
        deliver(&mut app, &mut out);
        assert_eq!(
            typed(&out),
            [format!(
                "{:?}: {NOTE}",
                SessionRef::Agent(AgentId("captain".into()))
            )]
        );
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
    /// sent is never sent again, and one held when the TUI quit goes out
    /// from the next.
    #[test]
    fn the_file_outlasts_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let mut app = app();
        app.github.merge_notes = MergeNotes::at(path.clone());
        app.tree.agents[0].status = AgentStatus::Running;
        merge(&mut app);

        let mut next = app_with_cache(&path);
        next.github
            .pull_requests
            .insert(WorktreeId("w".into()), Some(pr(STATE_MERGED)));
        let mut out = Vec::new();
        deliver(&mut next, &mut out);
        assert_eq!(typed(&out).len(), 1, "the held note goes out");

        let mut third = app_with_cache(&path);
        merge(&mut third);
        let mut out = Vec::new();
        deliver(&mut third, &mut out);
        assert!(out.is_empty(), "sent once, ever: {out:?}");
        app.tree.agents[0].status = AgentStatus::Finished;
        let mut out = Vec::new();
        deliver(&mut app, &mut out);
        assert!(out.is_empty(), "the first client sees it sent: {out:?}");
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
            sent_to: None,
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

    /// Two clients on one daemon share what each sent: a captain one of
    /// them typed a note into is sent no second by the other until it has
    /// taken the first.
    #[test]
    fn a_second_client_waits_for_the_captain_to_take_the_first_note() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let mut first = app_with_cache(&path);
        merge(&mut first);
        let mut out = Vec::new();
        deliver(&mut first, &mut out);
        assert_eq!(typed(&out).len(), 1);

        let mut second = app_with_cache(&path);
        second.github.merge_notes.notes.push(MergeNote {
            url: "https://github.com/o/r/pull/43".into(),
            project: ProjectId("p".into()),
            text: "second".into(),
            sent: false,
            sent_to: None,
        });
        second.github.merge_notes.save();
        let mut out = Vec::new();
        deliver(&mut second, &mut out);
        assert!(
            out.is_empty(),
            "the captain has not taken the first: {out:?}"
        );
        second.tree.agents[0].status_changed_at = 2;
        deliver(&mut second, &mut out);
        assert_eq!(
            typed(&out),
            [format!(
                "{:?}: second",
                SessionRef::Agent(AgentId("captain".into()))
            )]
        );
    }
}
