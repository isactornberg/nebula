// The contract between `nebula serve` and the web app.
//
// `nebula serve` is a daemon client like the TUI: it subscribes over the
// daemon's socket, keeps the entity tree, reads the screens of the board's
// rows the way the TUI does (`nebula_client::board`, `nebula_core::why`),
// and serves what it holds as JSON on 127.0.0.1. The Rust side mirrors these
// types with serde (`rename_all = "camelCase"`); a change here is a change
// there, in the same commit.
//
// Endpoints:
//
//   GET  /api/state                 -> State
//   GET  /api/events                -> text/event-stream; one `state` event
//                                      carrying a State on connect and after
//                                      every change (coalesced to at most one
//                                      every 100 ms). The whole State, never
//                                      a diff: it is a few dozen sessions.
//   POST /api/refresh               -> 202 | ApiError
//        Reads Linear's open issues assigned to you and the pull request on
//        each session's checkout again, or joins the refresh under way: the
//        State says it runs and, once it lands, what it read. The server
//        reads them on nothing else, no timer: the page asks once when it
//        opens on a State that has read nothing yet, and on Refresh.
//   POST /api/issues/:key/start     <- StartIssue  -> IssueStarted | ApiError
//        Hands the issue in a lane to the captain of `project`, which must
//        read its issues from the issue's Linear team, starting one when the
//        project has none, then moves the issue to its team's first started
//        state in Linear unless it is started already. 404 for an issue the
//        last refresh did not read, 409 for a project that does not read
//        that team, an issue handed over already unless `again`, a Start of
//        it under way, or a daemon that refuses the hand-off. A failed move
//        is not an error: the hand-off stands, and `moveFailed` says why.
//   POST /api/focus/rank            <- RankFocus  -> 202 | 204 | ApiError
//        Asks the daemon to have the orchestrator rank the focus picks, its
//        RANK TURN typed once it is at rest with an empty input box, held
//        until then: Re-rank. 202 too while a rank is under way, which it
//        joins. With `auto`, as the page opens, it asks only when the picks
//        are due (never set, or over three hours old with the candidates
//        changed since), fewer than four such asks were made today and the
//        orchestrator is at rest now, else 204. 409 with no orchestrator or
//        a daemon too old to ask.
//   POST /api/focus/feedback        <- FocusFeedback -> 202 | ApiError
//        Not today or Wrong on one of the orchestrator's picks, 404 for a
//        key that is none: the pick leaves the focus, Not today keeps the
//        key off the candidates until the day ends, and the next rank reads
//        what was said.
//   GET  /api/attention             -> Attention
//   POST /api/focus/picks           <- FocusPickSet[] -> PicksSet | ApiError
//        `nebula orchestrator attention --json` and `picks set --json -`:
//        for the orchestrator's own session on this machine alone, named
//        in an `X-Nebula-Agent` header, 403 for any other. Attention reads
//        Linear and the pull requests again first when they are over 10
//        minutes old. 400 for more than three picks, a key twice, a reason
//        missing or over 300 characters, and a key that is no candidate.
//   POST /api/schedules             <- AddSchedule -> ScheduleAdded | ApiError
//        Adds a schedule the daemon runs on its own: 400 for one with no
//        project, prompt or target, or a day that is none; 409 for one the
//        daemon refuses, saying why (a permission mode for a harness other
//        than Claude, the same session on a harness that reports no status
//        or resumes nothing, a time already past), or a daemon too old to
//        keep schedules.
//   POST /api/schedules/:id/remove  -> 202 | ApiError
//        Removes it, its runs with it: 404 for one there is not.
//   POST /api/schedules/:id/run     -> ScheduleRun | ApiError
//        Runs it now, as if it were due, leaving when it runs next as it
//        was, and answers with the run: 404 for one there is not.
//   POST /api/sessions              <- NewSession  -> 201 SessionStarted | ApiError
//        Starts a Claude session in the root checkout of `projectId`, one of
//        the State's projects, on the CLI's own model, named `agent-N` until
//        its first turn titles it, and types `text` into it as that turn,
//        with its `images` as a follow-up has them, once its input box is
//        up. 201 once the session reported taking the whole prompt, or with
//        `problem` saying why it may not have: the session stands either
//        way, for the page to open. 400 for text that cannot be typed or an
//        image id the server never gave, 404 for a project there is not or
//        an image whose file is gone, 409 for a daemon too old to say a
//        prompt arrived whole or to send an image.
//   GET  /api/sessions/:id          -> SessionDetail, read fresh each call
//   GET  /api/sessions/:id/turns    -> Turns, read fresh each call
//        The session's conversation, turn by turn, from its Claude
//        transcript: apart from the detail, which the Deck reads too, so
//        that never waits on a long transcript, and asked for only where
//        the thread shows.
//   GET  /api/sessions/:id/screen   -> Screen, read fresh each call
//   POST /api/images                <- the image's bytes   -> 201 Image | ApiError
//        Keeps an image for a follow-up, in the data dir's `attachments/`
//        for a week: a PNG, JPEG, GIF or WebP told by its first bytes,
//        whatever the Content-Type says, or a HEIC or HEIF, kept as the
//        JPEG macOS's `sips` makes of it. 413 over IMAGE_MAX_BYTES, 415
//        for anything else, and for a HEIC off a Mac.
//   POST /api/sessions/:id/send     <- FollowUp            -> 202 | ApiError
//        Types a follow-up, the way `nebula session send` does: refused with
//        409 while the session is running or has a dialog open. 202 once
//        the session reported taking the whole prompt (TypeTurn). Each image
//        goes as its file's path on a line of its own under the text, which
//        Claude Code takes for the image itself; 400 for an id the server
//        never gave, 404 for one whose file is gone, 409 for a daemon too
//        old to send one.
//   POST /api/sessions/:id/answer   <- { option: number }  -> 202 | ApiError
//        Picks option `option` (1-based) of the dialog open in the session.
//   POST /api/sessions/:id/show     -> 202 Shown | ApiError
//        Asks every TUI to put the session up ("Open in terminal"), and the
//        one the user touched last to bring its window to the front; 409
//        when no TUI is open to show it.
//   POST /api/sessions/:id/seen     -> 202 | ApiError
//        Marks a finish as seen, as putting it up in a TUI does. The app
//        sends it when it opens a finished session's detail.
//   POST /api/sessions/:id/archive  -> 202 | ApiError
//        Archives the session, as the TUI's Archive does: its agent stops
//        and it leaves the State's sessions, its worktree and branch kept.
//        A captain goes alone, its workers left running. 404 for a session
//        the State does not list.
//   POST /api/sessions/:id/unarchive -> 202 | ApiError
//        Brings an archived session back onto the list, as the TUI's
//        Unarchive does: the page's Undo of an archive. 404 for a session
//        that is not archived.
//   GET  /api/skills                -> Skills, from a scan of the disk at
//                                      most a few seconds old. Answered with
//                                      no daemon too, without its projects.
//   GET  /api/skills/usage          -> SkillUsage, from Claude Code's
//                                      transcripts, opening again only the
//                                      ones changed since the last read.
//                                      Apart from the list, so the list
//                                      never waits for it.
//   GET  /api/skills/:id            -> SkillDetail, its file read fresh
//   POST /api/skills/switch         <- SwitchSkills -> SkillWrite | ApiError
//   POST /api/skills/plugin         <- SwitchPlugin -> SkillWrite | ApiError
//   POST /api/skills/fix            <- FixSkills    -> SkillDone | ApiError
//        Acts on a fix by its id, one of the acts it offers: 404 for an id
//        the last scan did not give, 409 for one whose files changed since,
//        500 for an act that could not be made safely.
//   POST /api/skills/undo           <- UndoFix      -> SkillDone | ApiError
//        Puts back what an act did, by the token it answered, once, and
//        only where nothing is now: 404 for a token it does not hold.
//   GET  /api/skills/compare/:fix   -> SkillCompare, the fix's two copies
//                                      read now
//   POST /api/skills/check          <- CheckSkill   -> SkillCheck | ApiError
//        What the scan would say of a skill's file holding a text not yet
//        saved.
//   POST /api/skills/save           <- SaveSkill    -> SkillDone | ApiError
//        Writes a skill's file by the skill's id, while the file is at the
//        version its detail gave: 404 for an id the last scan did not give,
//        409 for a file that changed since or one the page does not edit,
//        413 for a text of 1 MiB or more, 500 for one it could not write.
//        Its Undo is an act's: `POST /api/skills/undo`.
//   POST /api/skills/delete         <- DeleteSkill  -> SkillDone | ApiError
//        Moves a skill to the Trash by its id, its real folder and each
//        link to it, while what goes is the version its detail gave: 404
//        for an id the last scan did not give, 409 for one that changed
//        since or one the page does not delete, 500 for one it could not
//        move. Its Undo is an act's: `POST /api/skills/undo`.
//   GET  /api/devices               -> Devices
//   POST /api/devices/pair          -> Devices | ApiError
//        Opens a pairing with a new one-time code, in place of any open
//        one, valid five minutes: 409 while Tailscale names no tailnet for
//        this machine.
//   POST /api/devices/:id/remove    -> Devices | ApiError
//        Removes a paired device, and its push subscription: 404 for one
//        that is not.
//   POST /api/devices/:id/test      -> PushTested | ApiError
//        Sends the device a test notification: 404 for a device that is
//        not paired, 409 for one with notifications off or whose push
//        service no longer knows it (which turns them off), 502 for one
//        its push service did not take.
//        The four are for this machine's own page: a device on the
//        tailnet is refused them with 403.
//   GET  /api/push                  -> Push
//   POST /api/push/subscribe        <- PushSubscription -> Push | ApiError
//        Keeps the subscription the device's browser made, in place of
//        any it had: 400 for one whose endpoint is not https:// or whose
//        keys do not read.
//   POST /api/push/unsubscribe      -> Push
//        The three are for a paired device: this machine's own page is
//        refused them with 404. The daemon pushes to every subscription
//        kept when a session turns red, once the Mac has had no input for
//        a minute.
//
// A device on the tailnet, through `tailscale serve`, gets none of these
// until it is paired: every path then answers 401, the API with an
// ApiError and any other with the server's own page that pairs it.
//
// Every error is an ApiError body with a 4xx/5xx status. Times are epoch
// milliseconds from the daemon's clock.

export type SessionStatus =
  | "fresh"
  | "running"
  | "finished"
  | "needsFeedback"
  | "terminated"
  | "disconnected";

export type Harness =
  | "claude"
  | "codex"
  | "cursor"
  | "pi"
  | "muse"
  | "grok"
  | "opencode"
  | "custom";

export type Role = "worker" | "orchestrator" | "captain";

export interface Project {
  id: string;
  name: string;
  /// The SPACE label the project carries (`Acme`), if any.
  space: string | null;
  /// The Linear team it reads its issues from (`ACME`), by its own `issues`
  /// setting or its space's, null for GitHub's. Start offers an issue to
  /// every project of its team.
  team: string | null;
}

export interface Session {
  id: string;
  name: string;
  projectId: string;
  projectName: string;
  branch: string;
  /// In the project's root checkout rather than a worktree of its own.
  rootCheckout: boolean;
  harness: Harness;
  role: Role;
  status: SessionStatus;
  statusChangedAt: number;
  /// A finish nobody has looked at yet.
  unseen: boolean;
  /// The line the board says under the row (`nebula_core::why`): what a
  /// waiting session asks, or how a finished turn ended. Null until read.
  why: string | null;
  /// `BLOCKED` or `QUESTION` when the finished turn's report line opens
  /// with one: such a finish waits on the user like a dialog.
  reportWord: "BLOCKED" | "QUESTION" | null;
  issueUrl: string | null;
  /// The issue's key (`ACME-900`, `#212`), read from `issueUrl`.
  issueKey: string | null;
  /// The open Linear issue assigned to you that the session works on, as
  /// the last refresh read it: the one `issueUrl` names, else the one whose
  /// key is in its branch as a whole word (`acme-123-fix` is ACME-123's), else
  /// the one Linear suggests its branch for.
  issue: Issue | null;
  /// The pull request on the session's checkout, as the last refresh read
  /// it: its branch's open one, else its newest while the checkout has
  /// nothing past that one's head.
  prUrl: string | null;
  prState: PrState | null;
}

export type PrState = "open" | "draft" | "merged" | "closed";

/// An open Linear issue assigned to you.
export interface Issue {
  /// `ACME-123`.
  key: string;
  url: string;
  title: string;
  /// Linear's: 0 none, 1 urgent, 2 high, 3 medium, 4 low.
  priority: number;
  /// The state's name (`In Progress`), and its type: `triage`, `backlog`,
  /// `unstarted` or `started`.
  state: string;
  stateType: string;
  assignee: string | null;
  /// The git branch Linear suggests for it.
  branchName: string;
  updatedAt: number;
  /// The team's key (`ACME`), and the Linear project it is in, if any.
  team: string;
  project: string | null;
  /// When Start handed it to a project's captain. Kept by the server
  /// across its restarts.
  started: IssueStart | null;
  /// The project Start last handed an issue of the same Linear project to,
  /// which it picks first.
  lastProject: string | null;
}

/// An issue Start handed to a project's captain.
export interface IssueStart {
  projectId: string;
  projectName: string;
  captainId: string;
  at: number;
  /// The brief waits for the captain to be at rest with an empty input
  /// box, as the daemon says now; false once it was typed.
  held: boolean;
  /// Why Linear was not moved to its started state, when it was not.
  moveFailed: string | null;
}

export interface StartIssue {
  /// The project's id.
  project: string;
  /// Start an issue handed to a captain already, again.
  again: boolean;
}

/// What a Start did.
export interface IssueStarted {
  captainId: string;
  held: boolean;
  /// The Linear state it was moved to; null when it was started already or
  /// the move failed.
  movedTo: string | null;
  moveFailed: string | null;
}

/// `inFlight`: started in Linear; `todo`: not started yet.
export type IssueLaneName = "inFlight" | "todo";

/// The assigned issues of one state no session works on, by priority
/// (urgent first, none last), then the latest changed first.
export interface IssueLane {
  lane: IssueLaneName;
  issues: Issue[];
}

/// Where the reading of Linear and the pull requests stands.
export interface Refresh {
  /// A refresh is under way.
  running: boolean;
  /// When the last one finished, null before the first.
  at: number | null;
  /// Why parts of the last one failed, a sentence each. What it could not
  /// read is kept as the refresh before read it.
  problems: string[];
}

export type SectionName = "needsYou" | "done" | "running" | "captains";

export interface BoardRow {
  sessionId: string;
  kind: "session" | "captain";
  /// For a captain: how many of its project's other sessions are at work.
  workers: number;
}

export interface BoardSection {
  section: SectionName;
  rows: BoardRow[];
}

/// The board as the TUI's orchestrator view draws it: sections in attention
/// order, empty ones left out, the orchestrator never a row. Under them the
/// lanes of the assigned issues no session works on, `inFlight` then
/// `todo`, an empty one left out.
export interface Board {
  sections: BoardSection[];
  lanes: IssueLane[];
  idle: number;
  stopped: number;
}

export interface FocusPick {
  /// The candidate's key: the issue's (`ACME-123`) when one is joined to it
  /// or it is an issue in a lane, else the session's id. By rule, always
  /// the session's id.
  key: string;
  /// The session it opens: its own, or the issue's most in need of you;
  /// null for an issue no session works on, which opens at `/i/<key>`.
  sessionId: string | null;
  reason: string;
  /// The answers the session's reply offers (`SessionDetail.answers`), so a
  /// focus card can show them without fetching the detail.
  answers: string[];
}

/// Up to three things worth the user's time. By "rule", the board's rows
/// that need you, then the ones done, each with its why line as the
/// reason, empty until read: never a running session, a captain at rest or
/// an issue, so with nothing that needs you `picks` is empty. By
/// "orchestrator", the picks it chose and explained from the candidates
/// (`Attention`), the rows that need you or are done and the issues in the
/// lanes, those that are candidates still and that you did not put aside;
/// rows that need you may be left out, and stay on the board.
export interface Focus {
  source: "rule" | "orchestrator";
  /// When the orchestrator set the picks shown, or the ones the rule
  /// stands in for; null while it never has.
  at: number | null;
  picks: FocusPick[];
  /// Why the rule stands in for the orchestrator's picks: `stale`, over
  /// three hours old with the candidates changed since; `gone`, none is a
  /// candidate now; `setAside`, you said Not today or Wrong to each;
  /// `unread`, Linear and the pull requests, which candidates are keyed
  /// by, are not read yet. Null by the orchestrator, and by rule while it
  /// never picked.
  fallback: "stale" | "gone" | "setAside" | "unread" | null;
  /// A rank asked of the orchestrator that has not set picks yet.
  rank: FocusRank | null;
}

export interface FocusRank {
  at: number;
  /// The page asked as it opened, not Re-rank.
  auto: boolean;
  /// `held`: waits for the orchestrator to be at rest with an empty input
  /// box; `ranking`: typed, and it is at work; `ended`: its turn ended
  /// without setting picks.
  state: "held" | "ranking" | "ended";
}

export interface RankFocus {
  auto: boolean;
}

export type Verdict = "notToday" | "wrong";

export interface FocusFeedback {
  key: string;
  verdict: Verdict;
  reason: string | null;
}

/// One pick `nebula orchestrator picks set` sets.
export interface FocusPickSet {
  key: string;
  reason: string;
}

export interface PicksSet {
  said: string;
}

/// What `nebula orchestrator attention --json` prints, compact, as every
/// rank reads it: absent fields are none, times are ages (`5m`, `3h`).
export interface Attention {
  /// How long ago Linear and the pull requests were read.
  read: string | null;
  problems?: string[];
  /// The most urgent 25, in lane order.
  candidates: Candidate[];
  /// How many more there are.
  more?: number;
  /// What you said of picks in the last week, newest first.
  feedback?: Said[];
}

export type Lane = "needsYou" | "done" | "inFlight" | "todo";

export interface Candidate {
  key: string;
  lane: Lane;
  issue?: {
    title: string;
    state: string;
    priority?: "urgent" | "high" | "medium" | "low";
    project?: string;
    changed: string;
    /// The project whose captain Start handed it to.
    handed?: string;
  };
  sessions?: {
    id: string;
    project: string;
    name: string;
    status: SessionStatus;
    why?: string;
    since: string;
    pr?: PrState;
  }[];
}

export interface Said {
  key: string;
  said: Verdict;
  reason?: string;
  /// The pick's reason, and what it was.
  picked: string;
  what: string;
  ago: string;
}

/// What the TUIs show. `State.tui` is null when no TUI is open, or when the
/// daemon is too old to say.
export interface TuiState {
  /// The session the TUI the user touched last has on screen, if any.
  sessionId: string | null;
}

/// The devices paired to reach the page over the tailnet, and the pairing
/// open for the next one.
export interface Devices {
  /// This machine's tailnet name, null while Tailscale is not running.
  tailnet: string | null;
  /// Newest first.
  devices: Device[];
  pairing: Pairing | null;
}

export interface Device {
  id: string;
  name: string;
  /// The Tailscale user who paired it, the only one it is let in as.
  login: string;
  pairedAt: number;
  /// When it was last let in, to the minute.
  lastUsed: number;
  /// It turned notifications on, and keeps a push subscription.
  push: boolean;
}

/// What a paired device's page turns notifications on with.
export interface Push {
  /// This machine's VAPID public key, base64url: the subscription's
  /// `applicationServerKey`.
  key: string;
  /// This device keeps a subscription.
  on: boolean;
}

/// A browser's push subscription, as `PushSubscription.toJSON()` has it.
export interface PushSubscription {
  endpoint: string;
  keys: { p256dh: string; auth: string };
}

/// What came of a test notification.
export interface PushTested {
  said: string;
}

/// A one-time code that pairs one device until `expiresAt`, the link that
/// carries it, and the link's QR code: a row of `1` (dark) and `0` (light)
/// for each line of modules, with no quiet zone.
export interface Pairing {
  code: string;
  url: string;
  qr: string[];
  expiresAt: number;
}

/// The `202` of a show.
/// The largest image `POST /api/images` takes, 20 MiB: a phone's photo is
/// 2 to 8 MB as a JPEG and up to about 15 MB at 48 megapixels.
export const IMAGE_MAX_BYTES = 20 * 1024 * 1024;

export interface Image {
  /// What a follow-up names the image by in `images`.
  id: string;
}

export interface FollowUp {
  text: string;
  /// Ids of images `POST /api/images` kept, sent in this order.
  images?: string[];
}

export interface Shown {
  /// The TUI the user touched last had the session on screen already.
  alreadyOnScreen: boolean;
}

export interface State {
  projects: Project[];
  /// Every live session, the orchestrator and captains included, in the
  /// palette's attention order: needs feedback, running, an unseen finish,
  /// the rest, each the most recently used first.
  sessions: Session[];
  board: Board;
  focus: Focus;
  /// The orchestrator session, once there is one: its thread is read and
  /// answered by the session endpoints, as any session's is.
  orchestratorId: string | null;
  tui: TuiState | null;
  refresh: Refresh;
  /// The day the file `today_file` in config.json names, as last read and
  /// as version 2 says it, a version 1 file made into one: null while that
  /// is not set, or the file is missing or unreadable.
  day: Day | null;
  /// Why that file could not be read as a day, a sentence, when it could
  /// not.
  dayProblem: string | null;
  schedules: Schedules;
  server: { version: string; protocol: number };
}

/// The daemon's schedules: a prompt in a project, or the orchestrator's
/// rank of the focus picks, run on its own on a form of time, with every
/// run it kept.
export interface Schedules {
  /// `schedules` in `config.json`: off, the default, the page shows no
  /// Schedules but its own page, which says how to turn them on.
  enabled: boolean;
  /// `enabled`, and the daemon keeps schedules: false too for one older
  /// than protocol 63.
  available: boolean;
  schedules: Schedule[];
  /// Every run kept, newest first, each schedule's newest 50.
  runs: ScheduleRun[];
  /// What the add form offers to launch.
  harnesses: HarnessChoice[];
  /// Claude's permission modes a schedule may name, past `default`.
  permissionModes: string[];
  /// Whether `nebula login-item on` has the daemon start at login, so the
  /// schedules run after a reboot with no TUI opened; null off macOS.
  loginItem: boolean | null;
}

export type ScheduleTask = "prompt" | "rank";

/// Where each run of a prompt goes: a new session in a new worktree or in
/// the root checkout, or one session the first run starts there, typed
/// each later run's prompt once it is at rest with an empty input box, or
/// with fresh context, its CLI started again on a new conversation.
export type ScheduleTarget = "newWorktree" | "root" | "session" | "freshSession";

export type Weekday = "mon" | "tue" | "wed" | "thu" | "fri" | "sat" | "sun";

/// Every N minutes; a time of day on the Mac's clock on some days; once.
export type ScheduleWhen =
  | { kind: "every"; minutes: number }
  | { kind: "at"; hour: number; minute: number; days: Weekday[] }
  | { kind: "once"; at: number };

export interface Schedule {
  id: string;
  name: string;
  task: ScheduleTask;
  /// A prompt's alone.
  projectId: string | null;
  projectName: string | null;
  prompt: string | null;
  /// The harness's registry id, `claude` or a custom one's.
  harness: string | null;
  /// Null: the CLI's own.
  model: string | null;
  effort: string | null;
  /// Claude's --permission-mode; null as nebula launches the harness.
  permissionMode: string | null;
  target: ScheduleTarget | null;
  when: ScheduleWhen;
  /// Null once a once schedule has run.
  nextAt: number | null;
  createdAt: number;
  /// A same-session schedule's session, once its first run started one.
  sessionId: string | null;
}

export type RunOutcome = "started" | "held" | "typed" | "restarted" | "skipped" | "failed";

export interface ScheduleRun {
  scheduleId: string;
  /// When it was due; null for a run asked for now.
  dueAt: number | null;
  at: number;
  /// `started` a new session; `held` waits for its session to be at rest
  /// with an empty input box; `typed` into it; `restarted` it on a new
  /// conversation.
  outcome: RunOutcome;
  sessionId: string | null;
  /// Why it was skipped or failed.
  why: string | null;
  /// Why it ran when it did, when that was not on time.
  note: string | null;
}

export interface HarnessChoice {
  id: string;
  label: string;
  models: string[];
  efforts: string[];
  /// It says when it is at rest, which a same-session run needs.
  reportsStatus: boolean;
  /// It resumes a conversation, which a same-session run keeping its
  /// context needs.
  resumes: boolean;
}

export interface AddSchedule {
  /// Empty: the prompt's first words, or `Morning rank`.
  name: string;
  task: ScheduleTask;
  projectId: string | null;
  prompt: string | null;
  harness: string | null;
  model: string | null;
  effort: string | null;
  permissionMode: string | null;
  target: ScheduleTarget | null;
  when: ScheduleWhen;
}

export interface ScheduleAdded {
  id: string;
}

/// A day as version 2 of the file `today_file` names has it: one list for
/// each surface it shows on, every word the writer's. The file is this
/// beside `version: 2`, and only `at` is needed; a field the server does
/// not know is passed over, and one that is empty or none is not served.
export interface Day {
  /// When it was written, epoch ms.
  at: number;
  /// The Today band's tiles.
  tiles: DayTile[];
  /// The words of the line at the list's foot, after `Today`.
  chip: DayWord[];
  /// The Deck's pills along its top.
  pills: DayPill[];
  /// The chips under Coming up.
  status: DayStatus[];
}

/// A word's colour: none is the colour of the text around it.
export type DayTone = "ok" | "late" | "error" | "muted";

export interface DayWord {
  text: string;
  tone?: DayTone;
  /// When what it says happened, epoch ms: the page adds how long ago once
  /// that is more than a day.
  at?: number;
}

export interface DayLine {
  text: string;
  /// Where it links to: the page ends a linked line on ` ↗`.
  url?: string;
  /// Said in its colour in place of the line's own look, as a quiet
  /// `Nothing due` is `muted`.
  tone?: DayTone;
}

export interface DayRow {
  text: string;
  url?: string;
  /// The date it is due, `2026-11-10`: the page says how many days late it
  /// is, `today`, or how soon.
  due?: string;
}

/// A bar `value` of `of` full, with its caption.
export interface DayMeter {
  value: number;
  of: number;
  text: string;
}

export interface DayTile {
  head: string;
  /// The name of one of the page's icons, drawn before the head in `tone`;
  /// one it does not have draws none.
  icon?: string;
  tone?: DayTone;
  /// After the head and ` · `, joined with `, `.
  sub?: DayWord[];
  /// A sentence said in place of the body, `main`, `meter` and `rows`.
  problem?: string;
  main?: DayLine;
  meter?: DayMeter;
  rows?: DayRow[];
  /// The tile's foot link, to the rest of what its rows are.
  more?: DayLine;
}

/// A pill with a `dot` is the one at the row's end that says how things
/// stand; the others are a label and words.
export interface DayPill {
  label?: string;
  url?: string;
  dot?: DayTone;
  /// Joined with ` · `.
  words: DayWord[];
}

export interface DayStatus {
  /// Said with ` · ` and the time of `at` after it, when there is one.
  text: string;
  /// The dot's colour.
  tone?: DayTone;
  at?: number;
  /// A sentence on a line of its own under the chips.
  problem?: string;
}

export interface DialogOption {
  number: number;
  label: string;
}

export interface Dialog {
  question: string;
  options: DialogOption[];
}

export interface Reply {
  /// The session's last reply, as Markdown when it came from the CLI's
  /// transcript, else the plain text of its screen.
  text: string;
  source: "transcript" | "screen";
  at: number | null;
}

export interface SessionDetail {
  session: Session;
  reply: Reply | null;
  /// The answers the reply's `NEXT: a | b | c` line offers, read by the rule
  /// the quick-answers mod for Claude Code draws its pills by, recommended
  /// first. Empty for a reply with none, and for every harness but Claude.
  answers: string[];
  /// The dialog open in the session, if any.
  dialog: Dialog | null;
  /// For a captain: its workers, in attention order.
  workerIds: string[];
}

/// One turn of a session's conversation, from its Claude transcript.
export interface Turn {
  /// `user`: the prompt as typed, a slash command as its name and its
  /// arguments, an image as `[Image #1]`, without the reminders and output
  /// the CLI wraps around it; a turn nebula typed, a brief or a merge note,
  /// is one too. `assistant`: the reply its user read for the turn, by the
  /// rule `SessionDetail.reply` is read by: the text of the turn's newest
  /// message, never a tool call or its result.
  role: "user" | "assistant";
  /// As written, Markdown.
  text: string;
  /// When the transcript stamped it.
  at: number | null;
}

export interface Turns {
  /// The newest turns, oldest first: at most 40 and 256 KiB of text, the
  /// newest always, its end alone when it is longer. Null for a session
  /// that is not Claude's, one with no transcript yet, and from a daemon
  /// older than protocol 65: the page shows `SessionDetail.reply` then.
  turns: Turn[] | null;
  /// The transcript holds turns older than these.
  more: boolean;
}

/// The page's New: a Claude session in a project, on a prompt.
export interface NewSession {
  projectId: string;
  text: string;
  /// Ids of images `POST /api/images` kept, sent in this order.
  images?: string[];
}

/// The `201` of a New.
export interface SessionStarted {
  /// The new session, to open.
  id: string;
  /// Why its first turn may not have arrived whole, a sentence: it was
  /// typed but never reported, or not typed at all, such as while the
  /// session asks whether to trust its folder. Null once it arrived whole.
  problem: string | null;
}

export interface Screen {
  rows: string[];
  cols: number;
  height: number;
}

/// The agent CLIs whose skills the Skills page reads, each from the folders
/// it reads them from itself.
export type SkillCli = "claude" | "codex" | "cursor";

/// Where a CLI finds a skill.
export type SkillScope =
  /// Claude Code's managed settings directory.
  | "managed"
  /// The user's own: `~/.claude/skills`, `~/.agents/skills`,
  /// `~/.codex/skills`, `~/.cursor/skills`, and their `commands`.
  | "personal"
  /// A project's root checkout.
  | "project"
  /// A `<subdir>/.claude` in a project, which Claude loads once it works
  /// in that folder.
  | "nested"
  /// The `.claude` of a directory a space's `add_dirs` names, which every
  /// Claude session of the space's projects gets with `--add-dir`.
  | "addedDir"
  /// A Claude plugin's, named `plugin:skill`.
  | "plugin"
  /// Synced from the claude.ai account into `~/.claude/skills/synced`.
  | "claudeAi"
  /// Codex's `/etc/codex/skills`.
  | "admin"
  /// Bundled with the CLI: Codex's `~/.codex/skills/.system`, Cursor's
  /// `~/.cursor/skills-cursor`.
  | "system";

export interface SkillLoad {
  cli: SkillCli;
  /// What that CLI calls it: the frontmatter `name`, else the folder's,
  /// with a plugin's name in front for a plugin's skill. A command's is its
  /// path under `commands/`, `:` for each folder.
  name: string;
  scope: SkillScope;
  /// Where the CLI finds it: the path it reads, a symlink's own when one
  /// leads there.
  path: string;
  /// The project whose sessions load it, for a project's or nested one.
  project: string | null;
  /// The space whose projects load it, for an added directory's.
  space: string | null;
  /// The skill Claude loads instead, by id, when it finds another under
  /// the same name that it ranks first; null when it uses this one.
  shadowedBy: string | null;
}

/// A setting that keeps a skill from a CLI, or from its model.
export interface SkillSwitch {
  cli: SkillCli;
  /// `off`, or one of Claude's `skillOverrides` in between: `nameOnly`
  /// lists it to Claude without its description, `userInvocableOnly`
  /// hides it from Claude and keeps it in the `/` menu.
  state: "off" | "nameOnly" | "userInvocableOnly";
  /// `skillOverrides`, `enabledPlugins` or `syncClaudeAiSkills` for
  /// Claude; `skills.config` or `skills.bundled` for Codex.
  setting: string;
  /// The file that says so; null for a plugin no settings file enables.
  file: string | null;
  /// The project that file belongs to; null for the user's own settings.
  project: string | null;
}

/// Where a switch the page sets stands: on, one of Claude's two in
/// between, or off.
export type SkillSetting = "on" | SkillSwitch["state"];

/// A switch the page can set for one CLI, and where it stands.
export interface SkillControl {
  cli: SkillCli;
  /// `skillOverrides` or, for a plugin's skill, `enabledPlugins` for
  /// Claude; `skills.config` for Codex, whose switch is on or off.
  setting: "skillOverrides" | "enabledPlugins" | "skills.config";
  /// The file it writes, as the CLI reads it: the user's settings for a
  /// skill it loads everywhere, a project's `.claude/settings.local.json`
  /// for one it loads there only.
  file: string;
  /// Where it stands in the sessions it switches.
  state: SkillSetting;
  /// For `enabledPlugins`, the plugin as that setting names it:
  /// `kit@market`, the id the plugin's switch takes.
  plugin: string | null;
  /// The project whose sessions it switches; null for every session.
  project: string | null;
}

export type SkillProblemKind =
  /// A link to a skill whose target is gone.
  | "brokenLink"
  /// A SKILL.md that is a link out of its own folder: never read.
  | "linkOutside"
  /// A file that cannot be read, or is not UTF-8 text.
  | "unreadable"
  /// Frontmatter that does not parse, which Claude loads as no fields.
  | "badFrontmatter"
  /// A frontmatter `name` other than its folder's.
  | "nameMismatch"
  /// A `name` the Agent Skills spec does not allow: over 64 characters,
  /// or more than lowercase letters, digits and single hyphens.
  | "badName"
  /// A `description` over the spec's 1,024 characters.
  | "longDescription"
  /// No `name` or `description`, which Codex needs both of.
  | "missingField"
  /// Another skill one CLI finds under the same name, saying which wins.
  | "duplicateName"
  /// A skill of the user's own that one CLI alone loads.
  | "oneCli";

export interface SkillProblem {
  kind: SkillProblemKind;
  /// What is wrong, as a sentence.
  text: string;
  /// Part of a fix the user kept off the list, so not one to count.
  kept: boolean;
}

/// One thing to fix, for every problem it causes: the broken links in one
/// folder, the skills one name is found under, what one skill's own file
/// gets wrong, or the skills one CLI alone loads.
export interface SkillFix {
  /// Stable while it is about the same skills: what `FixSkills` names it by.
  id: string;
  /// The kind of problem it fixes.
  kind: SkillProblemKind;
  /// What is wrong, in a few words: `3 broken links`, `deploy twice`.
  title: string;
  /// Where: `in shopfront`, `in Claude and Cursor, in shopfront`.
  place: string;
  /// What is wrong, as sentences.
  text: string;
  /// What fixes it, as a sentence.
  fix: string;
  /// The skills it is about, by id.
  skills: string[];
  /// What the page can do about it, in the order to offer them.
  acts: SkillFixAct[];
}

export type SkillAct =
  /// Remove a folder's broken links: each one looked at again first, and
  /// one that changed left alone.
  | "removeLinks"
  /// Remove the command a skill of its name hides, only while git holds it
  /// as it is.
  | "removeCommand"
  /// Keep it off the list until one of its files changes.
  | "keep"
  /// Put a kept fix back on the list.
  | "unkeep"
  /// Read its two copies side by side: `GET /api/skills/compare/:fix`.
  | "compare";

export interface SkillFixAct {
  act: SkillAct;
  /// For a removal, what it does in git, said before it is done.
  says: string | null;
  /// Why the page will not do it, and leaves it to the user.
  refused: string | null;
}

export interface GitRepo {
  /// The repository's top folder.
  root: string;
  /// Null on a detached HEAD.
  branch: string | null;
}

/// Where a skill comes from, which the list groups it by.
export type SkillSource =
  /// A folder in a git repository: `Skill.repo`.
  | { kind: "repo" }
  /// A folder in no repository, put there by hand.
  | { kind: "folder" }
  /// Installed by the `npx skills` installer, by its lock file
  /// (`~/.agents/.skill-lock.json`): `source` is the GitHub repository
  /// (`owner/repo`) and `path` the SKILL.md in it.
  | { kind: "installer"; source: string; url: string | null; path: string | null }
  /// A Claude plugin's: its name, the version installed, and the
  /// marketplace with what that comes from (a GitHub repository, a URL, or a
  /// folder on this machine).
  | { kind: "plugin"; plugin: string; version: string | null; marketplace: string; from: string | null }
  | { kind: "claudeAi" }
  | { kind: "bundled"; cli: SkillCli };

/// One skill: one real folder holding a SKILL.md, or one legacy command
/// file, however many links and CLIs reach it.
export interface Skill {
  /// Stable while the skill stays where it is: a hash of its real path.
  id: string;
  /// The frontmatter `name`, else the folder's (a command's: its name).
  name: string;
  /// The frontmatter `description`, else the body's first line, as
  /// Claude Code takes it.
  description: string | null;
  kind: "skill" | "command";
  /// The real folder (a command's real file), every symlink resolved; a
  /// broken link's own path.
  path: string;
  source: SkillSource;
  /// The git repository the real folder is in, if any.
  repo: GitRepo | null;
  /// Every place a CLI finds it, in the order each CLI ranks them.
  loads: SkillLoad[];
  switches: SkillSwitch[];
  /// What the page can switch, a CLI each: none for one it cannot, as
  /// Cursor, a skill another of its name hides, or one a managed setting
  /// decides.
  controls: SkillControl[];
  problems: SkillProblem[];
}

export interface Skills {
  /// By name.
  skills: Skill[];
  /// What to fix: broken links, then names found twice, then a skill's own
  /// file, then skills one CLI alone loads.
  fixes: SkillFix[];
  /// The fixes the user kept off the list, whose files have not changed
  /// since: each offers `unkeep`.
  kept: SkillFix[];
  /// The user's home, which the page shows paths under as `~`.
  home: string;
  /// The CLIs installed here, by their home folder: only these are read.
  clis: SkillCli[];
}

/// How often Claude Code used each skill in the last `days` days, read
/// from its transcripts and its subagents': a `Skill` tool call, or a slash
/// command named as Claude loads a skill. Codex and Cursor are not read.
export interface SkillUsage {
  days: number;
  /// Every skill used in those days, most used first.
  used: SkillUse[];
}

export interface SkillUse {
  /// The skill, by its id in `Skills`.
  id: string;
  count: number;
  /// When it was used last.
  last: number;
  /// The project that last use was in, a worktree's counting as its
  /// project's; null for a session in none.
  project: string | null;
}

export type FrontmatterValue =
  | string
  | number
  | boolean
  | null
  | FrontmatterValue[]
  | { [key: string]: FrontmatterValue };

export interface SkillField {
  key: string;
  value: FrontmatterValue;
}

export interface SkillFile {
  /// Under the skill's folder, `/`-separated.
  path: string;
  /// In bytes; 0 for a link.
  size: number;
  /// A symlink's target: it is listed, never followed.
  link: string | null;
}

export interface SkillDetail {
  skill: Skill;
  /// The file read: the real folder's SKILL.md, or the command's file.
  /// Null for a broken link.
  file: string | null;
  /// The frontmatter, field by field in the file's order.
  frontmatter: SkillField[];
  /// Why the frontmatter does not parse, when it does not.
  frontmatterError: string | null;
  /// The file after its frontmatter, as Markdown; null when it was not
  /// read, with `unread` saying why.
  body: string | null;
  unread: string | null;
  /// The files in the skill's folder, links listed and not followed.
  files: SkillFile[];
  /// More files than `files` holds.
  truncated: boolean;
  /// What the page needs to edit `file`; null when it does not edit it,
  /// with `uneditable` saying why.
  edit: SkillEdit | null;
  uneditable: string | null;
  /// What deleting the skill moves to the Trash; null when the page does
  /// not delete it, with `undeletable` saying why. A skill the page neither
  /// edits nor deletes for one reason has that one line in both.
  delete: SkillDelete | null;
  undeletable: string | null;
  /// What the repository `skill.repo` has not committed, in a few words:
  /// `4 changed files`. Null when nothing, when it is in none, or when git
  /// did not answer.
  changes: string | null;
}

export interface SkillEdit {
  /// The whole file as read, frontmatter included.
  text: string;
  /// A hash of its bytes, which `SaveSkill` sends back.
  version: string;
  /// What to know before editing it: that the skills installer's next
  /// update replaces it.
  note: string | null;
}

/// `POST /api/skills/check`: the text to check, for the skill `id`.
export interface CheckSkill {
  id: string;
  text: string;
}

/// What the scan would say of the skill's file holding that text.
export interface SkillCheck {
  problems: SkillProblem[];
  /// The text after its frontmatter, as the page renders a SKILL.md.
  body: string;
}

/// `POST /api/skills/save`: the skill's new text, byte for byte, and the
/// version of the text it was edited from.
export interface SkillDelete {
  /// Each path it moves to the Trash: the real folder (or command file)
  /// first, when there is one, then each link to it.
  goes: SkillGoes[];
  /// What git will show, a sentence for each repository, or that the Trash
  /// holds the only copy of a folder in none.
  says: string[];
  /// What else goes: the skills installer's entry for it.
  note: string | null;
  /// A fingerprint of what goes, which `DeleteSkill` sends back.
  version: string;
}

export interface SkillGoes {
  path: string;
  link: boolean;
}

export interface DeleteSkill {
  id: string;
  version: string;
}

export interface SaveSkill {
  id: string;
  version: string;
  text: string;
}

/// `POST /api/skills/switch`: set one CLI's switch for skills, by their
/// ids, all switched in one file. Codex's is `on` or `off`.
export interface SwitchSkills {
  cli: SkillCli;
  set: SkillTo[];
}

export interface SkillTo {
  id: string;
  state: SkillSetting;
}

/// `POST /api/skills/plugin`: turn a Claude plugin on or off, by its
/// `SkillControl.plugin`.
export interface SwitchPlugin {
  plugin: string;
  on: boolean;
}

/// What a switch wrote. The file is read right before it is written and
/// replaced only when it is still as read; every key and line it does not
/// change is kept, and nothing is committed.
export interface SkillWrite {
  /// The file written, a link's target when the setting's file is a link.
  file: string;
  /// The setting's file, when it is a link to `file`.
  link: string | null;
  /// False when the file said so already, and was not written.
  changed: boolean;
  /// The git repository `file` is in, in a few words: `dotfiles: 1 changed
  /// file, nothing committed`, or that git ignores it there.
  repo: string | null;
  /// Each skill's state before, which set again puts it back.
  before: SkillTo[];
}

/// `POST /api/skills/fix`: one of the acts a fix offers, by the fix's id.
export interface FixSkills {
  fix: string;
  act: Exclude<SkillAct, "compare">;
}

/// `POST /api/skills/undo`.
export interface UndoFix {
  undo: string;
}

/// What an act, or its Undo, did. Nothing is committed.
export interface SkillDone {
  /// What it did, naming the files: `Removed 28 links from acme-api's
  /// .claude/skills.`
  said: string;
  /// Each repository it changed a file in, in a few words: `acme-api: 28
  /// deleted files, nothing committed`, or that git ignores them.
  repos: string[];
  /// What it left as it was because it changed since the list was read,
  /// as sentences.
  skipped: string[];
  /// What `UndoFix` takes to put it back; null when there is nothing to.
  undo: string | null;
}

/// The two copies of one skill a fix is about.
export interface SkillCompare {
  files: SkillCompared[];
  /// The two are the same, byte for byte.
  same: boolean;
}

export interface SkillCompared {
  /// The skill, by id.
  skill: string;
  /// Its SKILL.md, the real file; null when it was not read.
  file: string | null;
  text: string | null;
  /// Why it was not read.
  unread: string | null;
}

export interface ApiError {
  error: string;
}
