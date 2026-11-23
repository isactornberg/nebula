import type {
  ApiError,
  Board,
  Issue,
  IssueLaneName,
  IssueStart,
  PrState,
  Project,
  Refresh,
  Schedule,
  Schedules,
  SectionName,
  Session,
  SessionDetail,
  Shown,
  State,
  TuiState,
  Verdict,
} from "./api/contract";

export type Route =
  | { kind: "idle" }
  | { kind: "session"; id: string }
  /// The orchestrator's thread, whichever session it is now, or that none
  /// runs.
  | { kind: "orchestrator" }
  | { kind: "issue"; key: string }
  /// New session.
  | { kind: "new" }
  | { kind: "skills" }
  /// What needs fixing, as a screen of its own on a phone.
  | { kind: "skillFixes" }
  | { kind: "skill"; id: string }
  | { kind: "schedules" }
  /// The add form.
  | { kind: "scheduleNew" }
  | { kind: "schedule"; id: string };

/// The page's sections, which its nav moves between.
export type Section = "attention" | "skills" | "schedules";

export function sectionOf(route: Route): Section {
  if (route.kind === "skills" || route.kind === "skillFixes" || route.kind === "skill") return "skills";
  if (route.kind === "schedules" || route.kind === "scheduleNew" || route.kind === "schedule") return "schedules";
  return "attention";
}

const ROUTES: [RegExp, (id: string) => Route][] = [
  [/^\/s\/([^/]+)\/?$/, (id) => ({ kind: "session", id })],
  [/^\/i\/([^/]+)\/?$/, (key) => ({ kind: "issue", key })],
  [/^\/skills\/([^/]+)\/?$/, (id) => ({ kind: "skill", id })],
  [/^\/schedules\/([^/]+)\/?$/, (id) => ({ kind: "schedule", id })],
];

export function parseRoute(pathname: string): Route {
  if (/^\/new\/?$/.test(pathname)) return { kind: "new" };
  if (/^\/orchestrator\/?$/.test(pathname)) return { kind: "orchestrator" };
  if (/^\/skills\/?$/.test(pathname)) return { kind: "skills" };
  // A skill's id is 16 hex digits, so it is never `fixes`.
  if (/^\/skills\/fixes\/?$/.test(pathname)) return { kind: "skillFixes" };
  if (/^\/schedules\/?$/.test(pathname)) return { kind: "schedules" };
  // A schedule's id is a ULID, so it is never `new`.
  if (/^\/schedules\/new\/?$/.test(pathname)) return { kind: "scheduleNew" };
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
    case "orchestrator":
      return "/orchestrator";
    case "issue":
      return `/i/${encodeURIComponent(route.key)}`;
    case "new":
      return "/new";
    case "skills":
      return "/skills";
    case "skillFixes":
      return "/skills/fixes";
    case "skill":
      return `/skills/${encodeURIComponent(route.id)}`;
    case "schedules":
      return "/schedules";
    case "scheduleNew":
      return "/schedules/new";
    case "schedule":
      return `/schedules/${encodeURIComponent(route.id)}`;
  }
}

/// A focus pick: the session it opens, or, for an issue no session works
/// on, the issue.
export type FocusItem = {
  kind: "focus";
  n: number;
  key: string;
  sessionId: string | null;
  issue: Issue | null;
  reason: string;
  answers: string[];
};

/// What the list shows: the board, every session at work, the issues no
/// session works on, or every session by its project; on a phone also the
/// day and the rest of the page, which a wide screen shows beside the list.
export type ListView = "inbox" | "running" | "issues" | "projects" | "today" | "more";

/// A row of the list: a session under its group's head (a board section,
/// `running`, a project's id, or `found` for a find's matches, the
/// orchestrator among them), or an issue in its lane. A `leaving` row is
/// one `keepDone` kept that folds away.
export type ListItem =
  | { kind: "row"; group: string; sessionId: string; captain: boolean; workers: number; orchestrator?: true; leaving?: true }
  | { kind: "issue"; lane: IssueLaneName; issue: Issue };

/// A Done row the list keeps where it was once opening its session read
/// it: the session, its place among Done's rows, and the finish it is for.
export type Held = { id: string; at: number; changed: number };

/// Where session `id` stands among the Done rows of `rows`, -1 if not there.
export function doneIndex(rows: ListItem[], id: string): number {
  return rows.filter((r) => r.kind === "row" && r.group === "done").findIndex((r) => r.kind === "row" && r.sessionId === id);
}

/// The Inbox's `rows` with `open`, the open session's Done row, back at its
/// place in Done once reading it took it out, as the TUI's board keeps the
/// session on screen there, and `left`, the one open last, there as a
/// `leaving` row; a captain's leaves its row under Captains alone. Neither
/// holds once its session starts another turn, or asks.
export function keepDone(rows: ListItem[], state: State, open: Held | null, left: Held | null): ListItem[] {
  const keep = (rows: ListItem[], h: Held | null, leaving: boolean): ListItem[] => {
    const s = h && state.sessions.find((x) => x.id === h.id);
    if (!h || !s || s.status !== "finished" || s.statusChangedAt !== h.changed) return rows;
    const now = rows.find((r) => r.kind === "row" && r.sessionId === h.id);
    if (now?.kind === "row" && now.group !== "captains") return rows;
    const rest = leaving ? rows : rows.filter((r) => r !== now);
    const done = rest.filter((r) => r.kind === "row" && r.group === "done").length;
    const first = rest.findIndex((r) => r.kind === "row" && r.group !== "needsYou");
    const at = first < 0 ? rest.length : first + Math.min(h.at, done);
    const row: ListItem = {
      kind: "row",
      group: "done",
      sessionId: h.id,
      captain: s.role === "captain",
      workers: now?.kind === "row" ? now.workers : 0,
      ...(leaving && { leaving: true as const }),
    };
    return [...rest.slice(0, at), row, ...rest.slice(at)];
  };
  return keep(keep(rows, open, false), left, true);
}

/// The orchestrator's picks, or the rule's, as the Spotlight shows them:
/// up to three, numbered, each a session or an issue the state still has.
export function focusPicks(state: State): FocusItem[] {
  const known = new Set(state.sessions.map((s) => s.id));
  const laned = state.board.lanes.flatMap((l) => l.issues);
  return state.focus.picks
    .flatMap((p) => {
      const issue = p.sessionId === null ? laned.find((i) => i.key === p.key) : undefined;
      if (p.sessionId === null ? !issue : !known.has(p.sessionId)) return [];
      return [{ key: p.key, sessionId: p.sessionId, issue: issue ?? null, reason: p.reason, answers: p.answers }];
    })
    .slice(0, 3)
    .map((p, i) => ({ kind: "focus", n: i + 1, ...p }));
}

/// The rows of `view`, in order, of project `project` alone when one is
/// picked: the board's rows section by section; every session at work, the
/// workers under a captain too; the issue lanes, of the projects reading
/// the issue's team; every session, project by project in the daemon's
/// order. The day and the rest have no rows.
export function viewItems(state: State, view: ListView, project: string | null): ListItem[] {
  const byId = new Map(state.sessions.map((s) => [s.id, s]));
  const mine = (id: string) => {
    const s = byId.get(id);
    return !!s && (project === null || s.projectId === project);
  };
  const workers = new Map(state.board.sections.flatMap((x) => x.rows.map((r) => [r.sessionId, r.workers] as const)));
  const row = (group: string, id: string): ListItem => ({
    kind: "row",
    group,
    sessionId: id,
    captain: byId.get(id)?.role === "captain",
    workers: workers.get(id) ?? 0,
  });
  switch (view) {
    case "inbox":
      return state.board.sections.flatMap((x) =>
        x.rows
          .filter((r) => mine(r.sessionId))
          .map((r) => ({
            kind: "row" as const,
            group: x.section,
            sessionId: r.sessionId,
            captain: r.kind === "captain",
            workers: r.workers,
          })),
      );
    case "running":
      return sessionOrder(state)
        .filter((id) => mine(id) && byId.get(id)!.status === "running")
        .map((id) => row("running", id));
    case "projects": {
      const order = sessionOrder(state).filter(mine);
      const ids = [...new Set([...state.projects.map((p) => p.id), ...order.map((id) => byId.get(id)!.projectId)])];
      return ids.flatMap((p) => order.filter((id) => byId.get(id)!.projectId === p).map((id) => row(p, id)));
    }
    case "issues": {
      const team = project === null ? null : (state.projects.find((p) => p.id === project)?.team ?? "");
      return state.board.lanes.flatMap((l) =>
        l.issues.filter((i) => team === null || i.team === team).map((issue) => ({ kind: "issue" as const, lane: l.lane, issue })),
      );
    }
    case "today":
    case "more":
      return [];
  }
}

/// How many rows need you: the Inbox's badge.
export function needsYouCount(state: State): number {
  return state.board.sections.find((x) => x.section === "needsYou")?.rows.length ?? 0;
}

/// Every session but the orchestrator: the board's rows section by
/// section, every one that needs you first, then the sessions the board
/// leaves off, idle or stopped, in the order the state sends them. The
/// Deck's tiles and the find's matches are in it.
export function sessionOrder(state: State): string[] {
  const known = new Set(state.sessions.map((s) => s.id));
  const rows = state.board.sections.flatMap((s) => s.rows.map((r) => r.sessionId)).filter((id) => known.has(id));
  const onBoard = new Set(rows);
  const rest = state.sessions.filter(
    (s) => !onBoard.has(s.id) && s.role !== "orchestrator" && s.id !== state.orchestratorId,
  );
  return [...rows, ...rest.map((s) => s.id)];
}

/// What the orchestrator is, under its name.
export const ORCHESTRATOR_LINE = "Reads, starts and steers the sessions of every project";

export const NO_ORCHESTRATOR = "No orchestrator runs: F10 in the TUI starts one.";

/// The orchestrator's session, while one runs.
export function orchestratorOf(state: State): Session | null {
  return state.sessions.find((s) => s.id === state.orchestratorId) ?? null;
}

/// The sessions whose name, project, branch or issue key holds every word
/// of `query`, case aside, in `sessionOrder`, those the board leaves off
/// too, and last the orchestrator. None while nothing is typed.
export function findItems(state: State, query: string): ListItem[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [];
  const byId = new Map(state.sessions.map((s) => [s.id, s]));
  const orchestrator = orchestratorOf(state);
  return [...sessionOrder(state), ...(orchestrator ? [orchestrator.id] : [])].flatMap((id) => {
    const s = byId.get(id)!;
    const hay = [s.name, s.projectName, s.branch, s.issueKey, s.issue?.key].join(" ").toLowerCase();
    if (!words.every((w) => hay.includes(w))) return [];
    const row: ListItem = { kind: "row", group: "found", sessionId: id, captain: s.role === "captain", workers: 0 };
    return [s === orchestrator ? { ...row, orchestrator: true as const } : row];
  });
}

export function itemKey(item: ListItem): string {
  return item.kind === "issue" ? `issue:${item.issue.key}` : `row:${item.sessionId}`;
}

/// Where opening `item` goes.
export function itemRoute(item: ListItem): Route {
  if (item.kind === "issue") return { kind: "issue", key: item.issue.key };
  return item.orchestrator ? { kind: "orchestrator" } : { kind: "session", id: item.sessionId };
}

/// Where opening pick `item` goes: its session, else its issue.
export function pickRoute(item: FocusItem): Route {
  return item.sessionId === null ? { kind: "issue", key: item.key } : { kind: "session", id: item.sessionId };
}

/// The board's rows that need you and are not among the picks: by the
/// orchestrator, it may leave some out, and the page says so.
export function unpicked(state: State, picks: FocusItem[]): (ListItem & { kind: "row" })[] {
  const picked = new Set(picks.map((p) => p.sessionId));
  return viewItems(state, "inbox", null).flatMap((i) =>
    i.kind === "row" && i.group === "needsYou" && !picked.has(i.sessionId) ? [i] : [],
  );
}

/// What a pick is, in a few words: its session's name, else its issue's
/// key and title.
export function pickWhat(item: FocusItem, state: State): string {
  const s = item.sessionId === null ? undefined : state.sessions.find((x) => x.id === item.sessionId);
  if (s) return s.name;
  return item.issue ? `${item.issue.key} ${item.issue.title}` : item.key;
}

/// What the page says once a pick is put aside.
export function setAsideNote(what: string, verdict: Verdict): string {
  return verdict === "notToday"
    ? `“${what}” is out of focus until tomorrow.`
    : `“${what}” is marked wrong, and the next rank reads why.`;
}

/// The line under Focus in the list: where a rank asked for stands, else
/// why the rule stands in for the orchestrator's picks.
export function focusNote(state: State, now: number): string | null {
  const { focus } = state;
  switch (focus.rank?.state) {
    case "held":
      return "Ranking once the orchestrator is at rest with an empty input box.";
    case "ranking":
      return "The orchestrator is ranking.";
    case "ended":
      return "The orchestrator's last rank ended without setting picks.";
  }
  const from = focus.at === null ? "" : ` from ${when(focus.at, now)}`;
  switch (focus.fallback) {
    case "stale":
      return `The orchestrator's picks${from} are over three hours old, and the board has changed since.`;
    case "gone":
      return `None of the orchestrator's picks${from} is on the board now.`;
    case "setAside":
      return `You put the orchestrator's picks${from} aside.`;
    case "unread":
      return `The orchestrator's picks${from} show once Linear and the pull requests are read.`;
  }
  if (focus.source === "orchestrator") return null;
  return state.orchestratorId ? "The orchestrator has not ranked yet." : "No orchestrator runs to rank them.";
}

/// A rank asked for is under way: Re-rank waits on it.
export function ranking(state: State): boolean {
  return state.focus.rank !== null && state.focus.rank.state !== "ended";
}

/// What the page says where the picks go when there are none: by rule,
/// that nothing needs you, which is then so; by the orchestrator, that it
/// picked nothing, as rows that need you may still be on the board.
export function noPicks(state: State): string {
  return state.focus.source === "orchestrator" ? "The orchestrator picked nothing." : "Nothing needs you.";
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

/// The on-screen keyboard is up: the visual viewport, not zoomed, is at
/// least 80 px shorter than the layout viewport the page is fixed to,
/// `layout`, the root's height, since iOS shrinks innerHeight with the
/// visual viewport in a Home Screen app.
export function keyboardUp(vv: { scale: number; height: number }, layout: number): boolean {
  return Math.abs(vv.scale - 1) <= 0.01 && layout - vv.height >= 80;
}

/// An IME is composing: its Enter, Esc and letters are its own. Safari sends
/// the Enter that confirms a word with `isComposing` false and keyCode 229.
export function composing(e: { isComposing: boolean; keyCode: number }): boolean {
  return e.isComposing || e.keyCode === 229;
}

type EnterKey = { key: string; shiftKey: boolean; altKey: boolean; metaKey: boolean; ctrlKey: boolean; isComposing: boolean; keyCode: number };

/// Whether a key in the composer sends it: Enter does, Shift+Enter adding a
/// line, except on a touch screen, whose Return adds a line and whose
/// button sends; ⌘↩ or Ctrl+↩ sends on either.
export function sendsOnEnter(e: EnterKey, touch: boolean): boolean {
  if (e.key !== "Enter" || composing(e)) return false;
  if (e.metaKey || e.ctrlKey) return true;
  return !touch && !e.shiftKey && !e.altKey;
}

export type KeyAction =
  | { kind: "move"; delta: 1 | -1 }
  | { kind: "open" }
  | { kind: "back" }
  | { kind: "terminal" }
  | { kind: "refresh" }
  | { kind: "start" }
  | { kind: "find" }
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
    case "/":
      return { kind: "find" };
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

/// Whether `1`-`9` answer the focus card of the row the cursor is on: only
/// while the cards are on screen, on Home, or atop the inbox on a screen
/// too narrow for Home.
export function cardsShown(route: Route, single: boolean, view: ListView, finding: boolean): boolean {
  return route.kind === "idle" && (!single || (view === "inbox" && !finding));
}

/// What the page asked last of a session, or of an issue as `issue:<key>`:
/// whether it is under way, and what went wrong. `seq` tells one ask from
/// the next.
export type Acting = { on: string | null; seq: number; busy: boolean; error: string | null };

export const NOT_ACTING: Acting = { on: null, seq: 0, busy: false, error: null };

/// Ask `seq` answered `error`, which counts only while it is the last ask.
export function ended(a: Acting, seq: number, error: string | null): Acting {
  return a.seq === seq ? { ...a, busy: false, error } : a;
}

/// What a page shows of the last ask: nothing on a page it was not about.
export function actingOn(a: Acting, on: string): { busy: boolean; error: string | null } {
  return a.on === on ? { busy: a.busy, error: a.error } : { busy: false, error: null };
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

/// A reply split into its body, the report line it ends on, if any, and
/// its closing `NEXT:` line, which goes when its answers are drawn as pills
/// and otherwise stands on its own after the report.
export function splitReply(
  text: string,
  dropNext: boolean,
): { body: string; report: { word: ReportWord; text: string } | null; next: string | null } {
  const lines = text.trimEnd().split("\n");
  let next: string | null = null;
  if (lines.at(-1)?.trim().startsWith("NEXT:")) {
    const line = lines.pop()!.trim();
    if (!dropNext) next = line;
    while (lines.length && !lines.at(-1)?.trim()) lines.pop();
  }
  const m = REPORT.exec(lines.at(-1)?.trim() ?? "");
  if (!m) return { body: lines.join("\n"), report: null, next };
  lines.pop();
  const rest = (m[2] ?? "").split("NEXT:")[0]!.trim();
  return { body: lines.join("\n").trimEnd(), report: { word: m[1] as ReportWord, text: rest }, next };
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

/// Who picked the `k` focus picks shown, and when.
export function focusLine(state: State, k: number): string {
  const { focus } = state;
  const at = focus.at === null ? "" : ` at ${clock(focus.at)}`;
  if (focus.source === "orchestrator") {
    return k === 0 ? `The orchestrator picked nothing${at}.` : `The orchestrator picked ${count(k)}${at}.`;
  }
  if (k === 0) return "";
  return k === 1
    ? "By rule: the board's first row that needs you or is done."
    : `By rule: the board's first ${count(k)} rows that need you or are done.`;
}

/// What the board holds, under the day: `3 need you · 2 done · 4 running`.
export function dayLine(board: Board): string {
  const n = (s: SectionName) => board.sections.find((x) => x.section === s)?.rows.length ?? 0;
  const parts = [
    [n("needsYou"), n("needsYou") === 1 ? "needs you" : "need you"],
    [n("done"), "done"],
    [n("running"), "running"],
  ] as const;
  const said = parts.filter(([k]) => k > 0).map(([k, word]) => `${k} ${word}`);
  return said.length ? said.join(" · ") : "No session is running.";
}

/// The colour of a dot that is not a session's: a schedule's run, a skill's
/// problem, a phone's notifications.
export type Tone = "red" | "yellow" | "blue" | "green" | "hollow";

export type Tint = "red" | "peach" | "yellow" | "green" | "blue" | "mauve" | "teal" | "plain";
export type Pill = { text: string; tint: Tint };

/// A session as the list and the Spotlight draw it: its dot's tone, the
/// word a screen reader hears for it, whether it pulses while at work,
/// and its pill, what it asks or the state of its pull request.
export type Look = {
  label: string;
  tone: "red" | "peach" | "blue" | "green" | "faint" | "hollow";
  pulse: boolean;
  pill: Pill | null;
};

const PR_TINT: Record<PrState, Tint> = { open: "green", draft: "plain", merged: "mauve", closed: "red" };

/// A session's pull request in a pill, in the TUI's words.
export function prPill(s: Session): Pill | null {
  if (!s.prUrl || !s.prState) return null;
  return { text: `${prLabel(s.prUrl)} ${prStateLabel(s.prState, true)}`, tint: PR_TINT[s.prState] };
}

export function look(s: Session): Look {
  const at = (label: string, tone: Look["tone"], pill: Pill | null = prPill(s), pulse = false): Look => ({
    label,
    tone,
    pulse,
    pill,
  });
  switch (s.status) {
    case "needsFeedback":
      return at("Dialog", "peach", { text: "Dialog", tint: "peach" });
    case "running":
      return at("Running", "green", prPill(s), true);
    case "finished":
      if (s.reportWord === "QUESTION") return at("Question", "red", { text: "Question", tint: "red" });
      if (s.reportWord === "BLOCKED") return at("Blocked", "red", { text: "Blocked", tint: "red" });
      if (s.unseen) return at("Done", "blue");
      return s.role === "captain" ? at("At rest", "hollow") : at("Finished", "faint");
    case "fresh":
      return at("Not started", "hollow", null);
    case "terminated":
      return at("Stopped", "hollow", { text: "Stopped", tint: "plain" });
    case "disconnected":
      return at("Disconnected", "hollow", null);
  }
}

/// The tint of a session's state pill, by its dot's tone.
export const TONE_TINT: Record<Look["tone"], string> = {
  red: "tint-red",
  peach: "tint-peach",
  blue: "tint-blue",
  green: "tint-green",
  faint: "tint-plain",
  hollow: "tint-plain",
};

const HARNESS: Record<Session["harness"], string> = {
  claude: "Claude",
  codex: "Codex",
  cursor: "Cursor",
  pi: "Pi",
  muse: "Muse",
  grok: "Grok",
  opencode: "OpenCode",
  custom: "Agent",
};

/// The CLI a session runs, by its name.
export function harnessName(s: Session): string {
  return HARNESS[s.harness];
}

/// The captain of a worker's project, if it has one.
export function captainOf(s: Session, state: State): Session | null {
  if (s.role !== "worker") return null;
  return state.sessions.find((c) => c.role === "captain" && c.projectId === s.projectId) ?? null;
}

/// A session's name where nothing beside it names its project: a captain
/// called `captain`, as every project's captain is, by the project it leads.
export function shownName(s: Session): string {
  return s.role === "captain" && s.name === "captain" ? `${s.projectName}'s captain` : s.name;
}

/// The branch a session's head names beside its project: its own
/// worktree's, unless that is the session's name. A root checkout is on the
/// project's own branch, its default as a rule, so it names none.
export function branchShown(s: Session): string | null {
  return !s.rootCheckout && s.branch && s.branch !== s.name ? s.branch : null;
}

/// Whom the composer says a reply goes to: `Reply to {who}…`, and
/// `Replying to {to}` over dictation.
export function replyWho(s: Session): { who: string; to: string } {
  if (s.role === "orchestrator") return { who: "the orchestrator", to: "the orchestrator" };
  const who = s.role === "captain" ? `${s.projectName}'s captain` : harnessName(s);
  return { who, to: shownName(s) === who ? who : `${who} · ${s.name}` };
}

/// What Archive asks before it stops a session, as the TUI's Archive does:
/// what goes, what stays and how it comes back, and for a captain, that its
/// workers, the project's other sessions as its Workers lists them, are
/// left running.
export function archiveAsk(s: Session, state: State): { title: string; says: string[] } {
  const says = ["The agent stops and the session leaves the list. Its worktree and branch stay: Undo brings it back, as does Unarchive in the TUI."];
  const workers = s.role === "captain" ? state.sessions.filter((w) => w.projectId === s.projectId && w.id !== s.id).length : 0;
  if (workers > 0) says.push(`Its ${workers === 1 ? "worker keeps" : `${workers} workers keep`} running: only the captain is archived.`);
  return { title: `Archive ${shownName(s)}?`, says };
}

/// An Archive the page asked for, from the moment it is asked until its
/// word goes: `failed` with the server's word for the archive or its Undo.
export interface Archived {
  id: string;
  name: string;
  seq: number;
  phase: "archiving" | "archived" | "undoing" | "back" | "failed";
  error: string | null;
}

/// What the page's foot says of an Archive, and whether it offers Undo.
export function archivedText(a: Archived): { text: string; undo: boolean } {
  switch (a.phase) {
    case "archiving":
      return { text: `Archiving ${a.name}…`, undo: false };
    case "archived":
    case "undoing":
      return { text: `Archived ${a.name}.`, undo: true };
    case "back":
      return { text: `Brought ${a.name} back.`, undo: false };
    case "failed":
      return { text: a.error ?? "", undo: false };
  }
}

const REPORT_WORD = /^[*_]{0,2}(DONE|BLOCKED|QUESTION)[*_]{0,2}\s*:?\s*/;

/// The line under a session's name in a row: its project, unless the row
/// is under its project's head, the issue's key
/// when its branch does not hold it, its branch unless it is its name or
/// it is a captain, a captain's workers at work, and the line the board
/// says of it, its report word left to its pill.
/// `text` without the report word it starts with, which a pill says.
export function stripReport(text: string): string {
  return text.replace(REPORT_WORD, "").trim();
}

export function rowMeta(s: Session, workers: number, project = true): string {
  if (s.role === "orchestrator") return ORCHESTRATOR_LINE;
  const key = sessionIssue(s)?.key;
  const parts = project ? [s.projectName] : [];
  if (key && !s.branch.toLowerCase().includes(key.toLowerCase())) parts.push(key);
  if (s.role === "captain") {
    if (s.name !== "captain") parts.push("captain");
    if (workers > 0) parts.push(workers === 1 ? "1 worker" : `${workers} workers`);
  } else if (s.branch && s.branch !== s.name) {
    parts.push(s.branch);
  }
  const why = s.why && stripReport(s.why);
  if (why && why !== s.name) parts.push(why);
  return parts.join(" · ");
}

/// The workers that finished today, newest first: what the day shipped.
export function shippedToday(state: State, now: number): Session[] {
  const today = new Date(now).toDateString();
  return state.sessions
    .filter(
      (s) =>
        s.role === "worker" &&
        s.status === "finished" &&
        s.reportWord === null &&
        new Date(s.statusChangedAt).toDateString() === today,
    )
    .sort((a, b) => b.statusChangedAt - a.statusChangedAt);
}

/// The schedules that run next, soonest first.
export function comingUp(schedules: Schedules, n = 3): Schedule[] {
  return schedules.schedules
    .filter((s) => s.nextAt !== null)
    .sort((a, b) => a.nextAt! - b.nextAt!)
    .slice(0, n);
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
