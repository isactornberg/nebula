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
//   GET  /api/skills                -> Skills, from a scan of the disk at
//                                      most a few seconds old. Answered with
//                                      no daemon too, without its projects.
//   GET  /api/skills/:id            -> SkillDetail, its file read fresh
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
/// them (a later slice), `source` is "rule": the board's rows that need
/// you, then the ones done, each with its why line as the reason, empty
/// until read. Running sessions and captains at rest are never picks, so
/// with nothing that needs you `picks` is empty.
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
}

/// One thing to fix, for every problem it causes: the broken links in one
/// folder, the skills one name is found under, what one skill's own file
/// gets wrong, or the skills one CLI alone loads.
export interface SkillFix {
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
  problems: SkillProblem[];
}

export interface Skills {
  /// By name.
  skills: Skill[];
  /// What to fix: broken links, then names found twice, then a skill's own
  /// file, then skills one CLI alone loads.
  fixes: SkillFix[];
  /// The user's home, which the page shows paths under as `~`.
  home: string;
  /// The CLIs installed here, by their home folder: only these are read.
  clis: SkillCli[];
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
}

export interface ApiError {
  error: string;
}
