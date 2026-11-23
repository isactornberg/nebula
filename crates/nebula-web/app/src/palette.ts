import type { Issue, Project, Session, State } from "./api/contract";
import { findItems, moveCursor, sessionOrder, shownName } from "./model";
import type { Theme } from "./theme";
import type { IconName } from "./ui";

/// What the palette does besides going to a session, an issue or a project.
export type ActionId =
  | "reply"
  | "terminal"
  | "show"
  | "seen"
  | "new"
  | "orchestrator"
  | "rank"
  | "refresh"
  | "follow"
  | "theme"
  | "schedules"
  | "skills"
  | "deck"
  | "phones";

export type PaletteItem =
  | { kind: "session"; key: string; session: Session }
  | { kind: "issue"; key: string; issue: Issue }
  | { kind: "project"; key: string; project: Project; sessions: number }
  | { kind: "action"; key: string; action: ActionId; label: string; icon: IconName; keys: string | null };

export type PaletteGroup = { title: "Recent" | "Sessions" | "Issues" | "Projects" | "Actions"; items: PaletteItem[] };

/// What the palette knows of the page beyond the state.
export type PaletteCtx = {
  /// The session open, if any.
  session: Session | null;
  /// Whether the page follows the TUI, null with no TUI open.
  follow: boolean | null;
  theme: Theme;
  /// The live terminal is shown.
  terminal: boolean;
  /// This machine pairs phones.
  phones: boolean;
};

/// How many sessions the palette offers before anything is typed.
const RECENT = 5;

export function queryWords(query: string): string[] {
  return query.toLowerCase().split(/\s+/).filter(Boolean);
}

const holds = (hay: string, words: string[]) => {
  const h = hay.toLowerCase();
  return words.every((w) => h.includes(w));
};

type Action = Omit<PaletteItem & { kind: "action" }, "kind" | "key"> & { also: string };

/// The actions on offer, the open session's first: each with the words it
/// is also found by.
function actions(state: State, ctx: PaletteCtx): Action[] {
  const s = ctx.session;
  const tui = ctx.follow !== null;
  const dark = ctx.theme === "dark";
  const all: (Action | false)[] = [
    !!s && { action: "reply", label: `Reply to ${shownName(s)}`, icon: "reply", keys: null, also: "answer follow-up send" },
    !!s && {
      action: "terminal",
      label: ctx.terminal ? "Hide the live terminal" : "Show the live terminal",
      icon: "terminal",
      keys: "T",
      also: "screen",
    },
    !!s && tui && { action: "show", label: "Show in TUI", icon: "monitor", keys: null, also: "put up" },
    !!s?.unseen && { action: "seen", label: "Mark seen", icon: "eye", keys: null, also: "read" },
    { action: "new", label: "New session", icon: "plus", keys: "N", also: "start claude prompt" },
    { action: "orchestrator", label: "Orchestrator", icon: "sparkle", keys: "O", also: "ask hand out work thread" },
    !!state.orchestratorId && { action: "rank", label: "Rank again", icon: "sparkle", keys: null, also: "focus picks orchestrator" },
    { action: "refresh", label: "Refresh Linear and pull requests", icon: "refresh", keys: "R", also: "issues reload" },
    tui && { action: "follow", label: ctx.follow ? "Stop following TUI" : "Follow TUI", icon: "terminal", keys: null, also: "" },
    {
      action: "theme",
      label: dark ? "Switch to the light theme" : "Switch to the dark theme",
      icon: dark ? "sun" : "moon",
      keys: null,
      also: "appearance colours",
    },
    state.schedules.enabled && { action: "schedules", label: "Schedules", icon: "clock", keys: null, also: "runs" },
    { action: "skills", label: "Skills", icon: "skills", keys: null, also: "" },
    { action: "deck", label: "Deck", icon: "deck", keys: null, also: "tiles stand" },
    ctx.phones && { action: "phones", label: "Phones", icon: "phone", keys: null, also: "pair devices" },
  ];
  return all.filter((a) => a !== false);
}

const sessionItem = (s: Session): PaletteItem => ({ kind: "session", key: `s:${s.id}`, session: s });

/// What the palette lists for `query`, group by group, each left out when
/// empty. With nothing typed, the sessions that changed last and every
/// action; else the sessions Find a session would find, every one on the
/// machine, the lanes' issues by key or title, the projects by name and the
/// actions by name, each by every word typed in any order.
export function paletteGroups(state: State, query: string, ctx: PaletteCtx): PaletteGroup[] {
  const words = queryWords(query);
  const acts: PaletteItem[] = actions(state, ctx)
    .filter((a) => holds(`${a.label} ${a.also}`, words))
    .map(({ also: _, ...a }) => ({ kind: "action", key: `a:${a.action}`, ...a }));
  const byId = new Map(state.sessions.map((s) => [s.id, s]));
  if (words.length === 0) {
    const recent = sessionOrder(state)
      .map((id) => byId.get(id)!)
      .filter((s) => s.id !== ctx.session?.id)
      .sort((a, b) => b.statusChangedAt - a.statusChangedAt)
      .slice(0, RECENT);
    return shown([
      { title: "Recent", items: recent.map(sessionItem) },
      { title: "Actions", items: acts },
    ]);
  }
  const sessions = findItems(state, query).flatMap((i) => (i.kind === "row" ? [byId.get(i.sessionId)!] : []));
  const issues = state.board.lanes.flatMap((l) => l.issues).filter((i) => holds(`${i.key} ${i.title}`, words));
  const projects = state.projects.filter((p) => holds(p.name, words));
  return shown([
    { title: "Sessions", items: sessions.map(sessionItem) },
    { title: "Issues", items: issues.map((issue) => ({ kind: "issue", key: `i:${issue.key}`, issue })) },
    {
      title: "Projects",
      items: projects.map((project) => ({
        kind: "project",
        key: `p:${project.id}`,
        project,
        sessions: state.sessions.filter((s) => s.projectId === project.id && s.role !== "orchestrator").length,
      })),
    },
    { title: "Actions", items: acts },
  ]);
}

const shown = (groups: PaletteGroup[]) => groups.filter((g) => g.items.length > 0);

/// `text` cut where the words typed are, each piece marked whether it is
/// one of them.
export function marks(text: string, words: string[]): { text: string; hit: boolean }[] {
  const lower = text.toLowerCase();
  // A letter whose lower case is longer would shift every mark after it.
  if (lower.length !== text.length || words.length === 0) return [{ text, hit: false }];
  const hit = Array.from({ length: text.length }, () => false);
  for (const w of words) {
    for (let at = lower.indexOf(w); at >= 0; at = lower.indexOf(w, at + w.length)) hit.fill(true, at, at + w.length);
  }
  const out: { text: string; hit: boolean }[] = [];
  for (let i = 0; i < text.length; i++) {
    const last = out.at(-1);
    if (last && last.hit === hit[i]) last.text += text[i];
    else out.push({ text: text[i]!, hit: hit[i]! });
  }
  return out;
}

export type PaletteKey = { kind: "move"; to: number | null } | { kind: "run"; at: number } | { kind: "close" };

/// What a key in the palette's field does, with the option at `active` of
/// `count` lit: the arrows move, Enter runs it, Esc closes.
export function paletteKey(key: string, active: number | null, count: number): PaletteKey | null {
  switch (key) {
    case "ArrowDown":
    case "ArrowUp":
      return { kind: "move", to: moveCursor(active, key === "ArrowDown" ? 1 : -1, count) };
    case "Enter":
      return count > 0 ? { kind: "run", at: active ?? 0 } : null;
    case "Escape":
      return { kind: "close" };
  }
  return null;
}

type Keyed = { key: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean; shiftKey: boolean };

/// What a key does on any page: ⌘K on a Mac, Ctrl+K elsewhere, the palette,
/// a bare `n` New session and a bare `o` the orchestrator's thread, which
/// the page takes only outside a text field. A Mac's Ctrl+K is its fields'
/// delete to the end of the line.
export function pageKey(e: Keyed, mac: boolean): "palette" | "new" | "orchestrator" | null {
  if (e.altKey || e.shiftKey) return null;
  if (e.metaKey || e.ctrlKey) return e.key.toLowerCase() === "k" && (mac ? e.metaKey : e.ctrlKey) ? "palette" : null;
  return e.key === "n" ? "new" : e.key === "o" ? "orchestrator" : null;
}

type Focusable = { isConnected: boolean; focus: (o?: FocusOptions) => void };

/// Focuses what had the focus before the palette opened, if it is still
/// on the page.
export function focusBack(el: Focusable | null) {
  if (el?.isConnected) el.focus({ preventScroll: true });
}
