import type {
  ApiError,
  Board,
  Issue,
  IssueLaneName,
  IssueStart,
  PrState,
  Project,
  Refresh,
  SectionName,
  Session,
  SessionDetail,
  Shown,
  State,
  TuiState,
} from "./api/contract";

export type Route =
  | { kind: "idle" }
  | { kind: "session"; id: string }
  | { kind: "issue"; key: string }
  | { kind: "skills" }
  /// What needs fixing, as a screen of its own on a phone.
  | { kind: "skillFixes" }
  | { kind: "skill"; id: string };

/// The page's sections, which its nav moves between.
export type Section = "attention" | "skills";

export function sectionOf(route: Route): Section {
  return route.kind === "skills" || route.kind === "skillFixes" || route.kind === "skill" ? "skills" : "attention";
}

const ROUTES: [RegExp, (id: string) => Route][] = [
  [/^\/s\/([^/]+)\/?$/, (id) => ({ kind: "session", id })],
  [/^\/i\/([^/]+)\/?$/, (key) => ({ kind: "issue", key })],
  [/^\/skills\/([^/]+)\/?$/, (id) => ({ kind: "skill", id })],
];

export function parseRoute(pathname: string): Route {
  if (/^\/skills\/?$/.test(pathname)) return { kind: "skills" };
  // A skill's id is 16 hex digits, so it is never `fixes`.
  if (/^\/skills\/fixes\/?$/.test(pathname)) return { kind: "skillFixes" };
  for (const [pattern, route] of ROUTES) {
    const m = pattern.exec(pathname);
    if (!m?.[1]) continue;
    try {
      return route(decodeURIComponent(m[1]));
    } catch {
      return { kind: "idle" };
    }
  }
  return { kind: "idle" };
}

export function routePath(route: Route): string {
  switch (route.kind) {
    case "idle":
      return "/";
    case "session":
      return `/s/${encodeURIComponent(route.id)}`;
    case "issue":
      return `/i/${encodeURIComponent(route.key)}`;
    case "skills":
      return "/skills";
    case "skillFixes":
      return "/skills/fixes";
    case "skill":
      return `/skills/${encodeURIComponent(route.id)}`;
  }
}

export type FocusItem = { kind: "focus"; n: number; sessionId: string; reason: string; answers: string[] };

export type ListItem =
  | FocusItem
  | { kind: "row"; section: SectionName; sessionId: string; captain: boolean; workers: number }
  | { kind: "issue"; lane: IssueLaneName; issue: Issue };

/// The left list top to bottom: the focus picks, numbered, then the board's
/// rows section by section, a pick's session left out of them, then the
/// issues no session works on, lane by lane.
export function listItems(state: State): ListItem[] {
  const known = new Set(state.sessions.map((s) => s.id));
  const focus: FocusItem[] = state.focus.picks
    .filter((p) => known.has(p.sessionId))
    .slice(0, 3)
    .map((p, i) => ({ kind: "focus", n: i + 1, sessionId: p.sessionId, reason: p.reason, answers: p.answers }));
  const picked = new Set(focus.map((f) => f.sessionId));
  const rows: ListItem[] = state.board.sections.flatMap((s) =>
    s.rows
      .filter((r) => known.has(r.sessionId) && !picked.has(r.sessionId))
      .map((r) => ({
        kind: "row" as const,
        section: s.section,
        sessionId: r.sessionId,
        captain: r.kind === "captain",
        workers: r.workers,
      })),
  );
  const issues: ListItem[] = state.board.lanes.flatMap((l) =>
    l.issues.map((issue) => ({ kind: "issue" as const, lane: l.lane, issue })),
  );
  return [...focus, ...rows, ...issues];
}

export function itemKey(item: ListItem): string {
  return item.kind === "issue" ? `issue:${item.issue.key}` : `${item.kind}:${item.sessionId}`;
}

/// Where opening `item` goes.
export function itemRoute(item: ListItem): Route {
  return item.kind === "issue" ? { kind: "issue", key: item.issue.key } : { kind: "session", id: item.sessionId };
}

/// The cursor one step from `from` (an index, or none yet), kept on the list.
export function moveCursor(from: number | null, delta: 1 | -1, length: number): number | null {
  if (length === 0) return null;
  if (from === null || from < 0) return delta > 0 ? 0 : length - 1;
  return Math.min(length - 1, Math.max(0, from + delta));
}

/// Where the cursor goes when `to` opens: it stays on a row that opens it,
/// else lands on the first that does, else leaves the list.
export function cursorFor(items: ListItem[], cursor: string | null, to: Route): string | null {
  const path = routePath(to);
  const opens = (i: ListItem) => routePath(itemRoute(i)) === path;
  const at = items.find((i) => itemKey(i) === cursor);
  if (at && opens(at)) return cursor;
  const first = items.find(opens);
  return first ? itemKey(first) : null;
}

export type KeyAction =
  | { kind: "move"; delta: 1 | -1 }
  | { kind: "open" }
  | { kind: "back" }
  | { kind: "terminal" }
  | { kind: "refresh" }
  | { kind: "start" }
  | { kind: "pick"; n: number };

export function keyAction(e: {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
}): KeyAction | null {
  if (e.metaKey || e.ctrlKey || e.altKey) return null;
  switch (e.key) {
    case "j":
    case "ArrowDown":
      return { kind: "move", delta: 1 };
    case "k":
    case "ArrowUp":
      return { kind: "move", delta: -1 };
    case "Enter":
      return { kind: "open" };
    case "Escape":
      return { kind: "back" };
    case "t":
      return { kind: "terminal" };
    case "r":
      return { kind: "refresh" };
    case "s":
      return { kind: "start" };
  }
  return /^[1-9]$/.test(e.key) ? { kind: "pick", n: Number(e.key) } : null;
}

/// Can a follow-up be typed into `s` now? The server refuses one while it
/// runs, and a session whose process is gone takes none.
export function canSend(s: Session): boolean {
  return s.status !== "running" && s.status !== "terminated" && s.status !== "disconnected";
}

/// The answer pills to draw: none while a dialog is open or nothing can be sent.
export function answersShown(d: SessionDetail): string[] {
  return d.dialog || !canSend(d.session) ? [] : d.answers;
}

/// A focus card's answer pills, by the same rule: a session that needs
/// feedback has a dialog open.
export function cardAnswers(item: FocusItem, s: Session): string[] {
  return s.status === "needsFeedback" || !canSend(s) ? [] : item.answers;
}

export type Pick = { kind: "option"; option: number } | { kind: "answer"; text: string };

/// What key or pill `n` picks: the open dialog's option `n`, else answer `n`.
export function pickFor(n: number, d: SessionDetail): Pick | null {
  if (d.dialog) {
    return d.dialog.options.some((o) => o.number === n) ? { kind: "option", option: n } : null;
  }
  const text = answersShown(d)[n - 1];
  return text === undefined ? null : { kind: "answer", text };
}

/// What the page says for a moment beside a control it was asked to use.
export interface Flash {
  text: string;
  error: boolean;
}

/// What Open in terminal says: whether the TUI put the session up or had
/// it up already, else why it could not.
export function showFlash(got: Shown | ApiError): Flash {
  if ("error" in got) return { text: got.error, error: true };
  return { text: got.alreadyOnScreen ? "Already on screen in the TUI." : "Shown in the TUI.", error: false };
}

/// What switching Follow TUI on says when the page is where it would take
/// it already: nothing when the page moves.
export function followFlash(route: Route, tui: TuiState): Flash | null {
  const there = route.kind === "session" ? route.id === tui.sessionId : tui.sessionId === null;
  if (!there) return null;
  return {
    text: tui.sessionId === null ? "The TUI shows no session." : "Already on the session the TUI shows.",
    error: false,
  };
}

export type ReportWord = "DONE" | "BLOCKED" | "QUESTION";

const REPORT = /^[*_]{0,2}(DONE|BLOCKED|QUESTION)(?![\p{L}\p{N}])[\s:*_-]*(.*)$/u;

/// A reply split into its body and the report line it ends on, if any. The
/// closing `NEXT:` line goes when its answers are drawn as pills.
export function splitReply(
  text: string,
  dropNext: boolean,
): { body: string; report: { word: ReportWord; text: string } | null } {
  const lines = text.trimEnd().split("\n");
  if (dropNext && lines.at(-1)?.trim().startsWith("NEXT:")) {
    lines.pop();
    while (lines.length && !lines.at(-1)?.trim()) lines.pop();
  }
  const m = REPORT.exec(lines.at(-1)?.trim() ?? "");
  if (!m) return { body: lines.join("\n"), report: null };
  lines.pop();
  const rest = (m[2] ?? "").split("NEXT:")[0]!.trim();
  return { body: lines.join("\n").trimEnd(), report: { word: m[1] as ReportWord, text: rest } };
}

/// How long ago `at` was, the way the TUI's board says it.
export function age(at: number, now: number): string {
  if (at <= 0) return "";
  const s = Math.floor(Math.max(0, now - at) / 1000);
  if (s < 60) return "now";
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

export function clock(at: number): string {
  return new Date(at).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
}

/// `at` as a clock time, with the day in front when it is not today.
export function when(at: number, now: number): string {
  const d = new Date(at);
  if (d.toDateString() === new Date(now).toDateString()) return clock(at);
  return `${d.toLocaleDateString("en-GB", { day: "numeric", month: "short" })} ${clock(at)}`;
}

export function longDate(now: number): string {
  const d = new Date(now);
  const part = (o: Intl.DateTimeFormatOptions) => d.toLocaleDateString("en-GB", o);
  return `${part({ weekday: "long" })} ${part({ day: "numeric" })} ${part({ month: "long" })}`;
}

const WORDS = ["no", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];
const count = (n: number) => WORDS[n] ?? String(n);
const capital = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

/// The idle page's one line: who picked the `k` focus picks shown, and what
/// the board holds.
export function summary(state: State, k: number): string {
  const { focus, board } = state;
  const picked =
    k === 0
      ? ""
      : focus.source === "orchestrator"
        ? `The orchestrator picked ${count(k)}${focus.at ? ` at ${clock(focus.at)}` : ""}. `
        : `Focus is the board's first ${k === 1 ? "row" : `${count(k)} rows`}, by rule. `;
  return picked + boardSentence(board);
}

export function boardSentence(board: Board): string {
  const n = (s: SectionName) => board.sections.find((x) => x.section === s)?.rows.length ?? 0;
  const parts = (
    [
      [n("needsYou"), "needs you", "need you"],
      [n("done"), "is done", "are done"],
      [n("running"), "is running", "are running"],
    ] as const
  ).filter(([k]) => k > 0);
  if (parts.length === 0) return "No session is running.";
  const said = parts.map(
    ([k, one, many], i) => `${count(k)}${i === 0 ? (k === 1 ? " session" : " sessions") : ""} ${k === 1 ? one : many}`,
  );
  const last = said.pop()!;
  return `${capital(said.length ? `${said.join(", ")} and ${last}` : last)}.`;
}

export type Tone = "red" | "yellow" | "blue" | "green" | "pink" | "hollow";

export function statusLook(s: Session): { label: string; tone: Tone } {
  switch (s.status) {
    case "needsFeedback":
      return { label: "Needs you", tone: "red" };
    case "running":
      return { label: "Running", tone: "yellow" };
    case "finished":
      if (s.reportWord === "QUESTION") return { label: "Needs you", tone: "red" };
      if (s.reportWord === "BLOCKED") return { label: "Blocked", tone: "red" };
      return s.unseen ? { label: "Done", tone: "blue" } : { label: "Finished", tone: "green" };
    case "fresh":
      return { label: "Not started", tone: "hollow" };
    case "terminated":
      return { label: "Stopped", tone: "pink" };
    case "disconnected":
      return { label: "Disconnected", tone: "hollow" };
  }
}

/// How long ago `at` was, in words: `just now`, `4 min ago`, `2 hours ago`.
export function ago(at: number, now: number): string {
  const min = Math.floor(Math.max(0, now - at) / 60_000);
  if (min < 1) return "just now";
  if (min < 60) return `${min} min ago`;
  const hours = Math.floor(min / 60);
  if (hours < 24) return hours === 1 ? "1 hour ago" : `${hours} hours ago`;
  const days = Math.floor(hours / 24);
  return days === 1 ? "1 day ago" : `${days} days ago`;
}

/// What the page says beside Refresh.
export function refreshed(r: Refresh, now: number): string {
  if (r.running) return "Refreshing…";
  return r.at === null ? "Not read yet" : `Updated ${ago(r.at, now)}`;
}

const PRIORITY = ["No priority", "Urgent", "High", "Medium", "Low"];

/// Linear's word for priority `p`.
export function priorityLabel(p: number): string {
  return PRIORITY[p] ?? PRIORITY[0]!;
}

/// The tone of a Linear state's dot, by its type: started is under way.
export function stateTone(stateType: string): Tone {
  return stateType === "started" ? "yellow" : "hollow";
}

/// A pull request's state in the TUI's words: an open one is `ready for
/// review`, GitHub's own phrase for the step out of draft, cut to `ready`
/// where it is `short` of room, as the TUI's sidebar badge cuts it.
export function prStateLabel(state: PrState, short = false): string {
  return state === "open" ? (short ? "ready" : "ready for review") : state;
}

/// The issue a session's row names: the Linear issue it works on, else the
/// one its issue URL names.
export function sessionIssue(s: Session): { key: string; url: string } | null {
  if (s.issue) return { key: s.issue.key, url: s.issue.url };
  return s.issueKey && s.issueUrl ? { key: s.issueKey, url: s.issueUrl } : null;
}

export function issueSite(url: string): string {
  if (url.includes("linear.app")) return "Linear";
  if (url.includes("github.com")) return "GitHub";
  return "Issue";
}

export function prLabel(url: string): string {
  const m = /\/pull\/(\d+)/.exec(url);
  return m ? `PR #${m[1]}` : "PR";
}

/// The projects Start offers `issue` to, every one that reads its issues
/// from the issue's Linear team, and the one it picks first: the one last
/// used for the issue's Linear project, else the first.
export function startOffer(projects: Project[], issue: Issue): { projects: Project[]; pick: string | null } {
  const offered = projects.filter((p) => p.team === issue.team);
  const last = offered.find((p) => p.id === issue.lastProject);
  return { projects: offered, pick: (last ?? offered[0])?.id ?? null };
}

/// What the page says of an issue Start handed over.
export function handedLine(started: IssueStart, now: number): string {
  const handed = `Handed to the captain of ${started.projectName} at ${when(started.at, now)}.`;
  return started.held
    ? `${handed} The brief waits until the captain is at rest with an empty input box.`
    : `${handed} The page goes to its worker once the captain starts it.`;
}
