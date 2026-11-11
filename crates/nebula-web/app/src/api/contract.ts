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
//   GET  /api/sessions/:id          -> SessionDetail, read fresh each call
//   GET  /api/sessions/:id/screen   -> Screen, read fresh each call
//   POST /api/sessions/:id/send     <- { text: string }    -> 202 | ApiError
//        Types a follow-up, the way `nebula session send` does: refused with
//        409 while the session is running or has a dialog open. 202 once
//        the session reported taking the whole prompt (TypeTurn).
//   POST /api/sessions/:id/answer   <- { option: number }  -> 202 | ApiError
//        Picks option `option` (1-based) of the dialog open in the session.
//   POST /api/sessions/:id/show     -> 202 Shown | ApiError
//        Asks every TUI to put the session up ("Open in terminal"), and the
//        one the user touched last to bring its window to the front; 409
//        when no TUI is open to show it.
//   POST /api/sessions/:id/seen     -> 202 | ApiError
//        Marks a finish as seen, as putting it up in a TUI does. The app
//        sends it when it opens a finished session's detail.
//   POST /api/orchestrator/send     <- { text: string }    -> 202 | ApiError
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
  /// The SPACE label the project carries (`Relox`), if any.
  space: string | null;
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
  /// The issue's key (`REL-900`, `#212`), read from `issueUrl`.
  issueKey: string | null;
  prUrl: string | null;
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
/// order, empty ones left out, the orchestrator never a row.
export interface Board {
  sections: BoardSection[];
  idle: number;
  stopped: number;
}

export interface FocusPick {
  sessionId: string;
  reason: string;
  /// The answers the session's reply offers (`SessionDetail.answers`), so a
  /// focus card can show them without fetching the detail.
  answers: string[];
}

/// Up to three things worth the user's time. Until the orchestrator picks
/// them (a later slice), `source` is "rule": the board's first rows, needs
/// you first, each with its why line as the reason, empty until read.
export interface Focus {
  source: "rule" | "orchestrator";
  at: number | null;
  picks: FocusPick[];
}

/// What the TUIs show. `State.tui` is null when no TUI is open, or when the
/// daemon is too old to say.
export interface TuiState {
  /// The session the TUI the user touched last has on screen, if any.
  sessionId: string | null;
}

/// The `202` of a show.
export interface Shown {
  /// The TUI the user touched last had the session on screen already.
  alreadyOnScreen: boolean;
}

export interface State {
  projects: Project[];
  /// Every live session, the orchestrator and captains included.
  sessions: Session[];
  board: Board;
  focus: Focus;
  /// The orchestrator session, once there is one.
  orchestratorId: string | null;
  tui: TuiState | null;
  /// agentos's Today, a later slice: null until then.
  today: null;
  server: { version: string; protocol: number };
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
  /// Claude Code's quick-answers mod draws its pills by, recommended first.
  /// Empty for a reply with none, and for every harness but Claude.
  answers: string[];
  /// The dialog open in the session, if any.
  dialog: Dialog | null;
  /// For a captain: its workers, in attention order.
  workerIds: string[];
}

export interface Screen {
  rows: string[];
  cols: number;
  height: number;
}

export interface ApiError {
  error: string;
}
