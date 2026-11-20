import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { Issue, Session, State } from "./api/contract";
import { findItems } from "./model";
import { focusBack, marks, pageKey, paletteGroups, paletteKey, type PaletteCtx, type PaletteItem } from "./palette";
import { Palette } from "./PaletteDialog";
import { NO_SCHEDULES } from "./schedules";

const MIN = 60_000;
const NOW = 100 * 86_400_000;

const session = (id: string, over: Partial<Session> = {}): Session => ({
  id,
  name: id,
  projectId: "p-shop",
  projectName: "shopfront",
  branch: "main",
  rootCheckout: false,
  harness: "claude",
  role: "worker",
  status: "finished",
  statusChangedAt: NOW - 60 * MIN,
  unseen: false,
  why: null,
  reportWord: null,
  issueUrl: null,
  issueKey: null,
  issue: null,
  prUrl: null,
  prState: null,
  ...over,
});

const issue = (key: string, title: string): Issue => ({
  key,
  url: `https://linear.app/acme/issue/${key}`,
  title,
  priority: 2,
  state: "Todo",
  stateType: "unstarted",
  assignee: "Ada",
  branchName: key.toLowerCase(),
  updatedAt: 1,
  team: "ACME",
  project: null,
  started: null,
  lastProject: null,
});

/// Seven sessions, one of them the orchestrator, and two issues in the lanes.
const state = (over: Partial<State> = {}): State => ({
  projects: [
    { id: "p-shop", name: "shopfront", space: null, team: "ACME" },
    { id: "p-api", name: "api", space: null, team: "ACME" },
  ],
  sessions: [
    session("cart", { name: "Persist the cart", branch: "acme-940-cart", statusChangedAt: NOW - 5 * MIN }),
    session("login", { name: "Fix the login form", status: "needsFeedback", statusChangedAt: NOW - 30 * MIN }),
    session("key", { name: "Rotate the staging key", projectId: "p-api", projectName: "api", statusChangedAt: NOW - 2 * MIN }),
    session("docs", { name: "Tidy the README", statusChangedAt: NOW - 90 * MIN }),
    session("bill", { name: "Port the billing page", status: "terminated", statusChangedAt: NOW - 1 * MIN }),
    session("perf", { name: "Cache the results", projectId: "p-api", projectName: "api", statusChangedAt: NOW - 120 * MIN }),
    session("orch", { name: "orchestrator", role: "orchestrator", statusChangedAt: NOW }),
  ],
  board: {
    sections: [
      { section: "needsYou", rows: [{ sessionId: "login", kind: "session", workers: 0 }] },
      { section: "done", rows: [{ sessionId: "cart", kind: "session", workers: 0 }] },
    ],
    lanes: [
      { lane: "todo", issues: [issue("ACME-950", "Cache the cart totals"), issue("ACME-951", "Sign the release builds")] },
    ],
    idle: 3,
    stopped: 1,
  },
  focus: { source: "rule", at: null, picks: [], fallback: null, rank: null },
  orchestratorId: "orch",
  tui: null,
  refresh: { running: false, at: NOW, problems: [] },
  today: null,
  todayProblem: null,
  schedules: NO_SCHEDULES,
  server: { version: "test", protocol: 1 },
  ...over,
});

const CTX: PaletteCtx = { session: null, follow: null, theme: "dark", terminal: false, phones: false };

const keyOf = (i: PaletteItem) => i.key;
const shown = (s: State, query: string, ctx: PaletteCtx = CTX) =>
  paletteGroups(s, query, ctx).map((g) => [g.title, g.items.map(keyOf)] as const);
const actionsOf = (ctx: PaletteCtx, s = state()) =>
  paletteGroups(s, "", ctx)
    .find((g) => g.title === "Actions")!
    .items.map((i) => (i.kind === "action" ? i.label : ""));

describe("the palette's results", () => {
  it("offers the sessions that changed last and every action before anything is typed", () => {
    const groups = shown(state(), "");
    expect(groups.map(([t]) => t)).toEqual(["Recent", "Actions"]);
    // Newest first, five at most, never the orchestrator.
    expect(groups[0]![1]).toEqual(["s:bill", "s:key", "s:cart", "s:login", "s:docs"]);
  });

  it("leaves the open session out of the recent ones", () => {
    const open = state().sessions.find((s) => s.id === "bill")!;
    expect(shown(state(), "", { ...CTX, session: open })[0]![1]).toEqual(["s:key", "s:cart", "s:login", "s:docs", "s:perf"]);
  });

  it("finds sessions by Find a session's rule, then issues, projects and actions, each group in that order", () => {
    const s = state();
    const groups = shown(s, "cart");
    expect(groups.map(([t]) => t)).toEqual(["Sessions", "Issues"]);
    // The same sessions, in the same order, as the list's own find.
    expect(groups[0]![1]).toEqual(findItems(s, "cart").map((i) => (i.kind === "row" ? `s:${i.sessionId}` : "")));
    expect(groups[1]![1]).toEqual(["i:ACME-950"]);
  });

  it("wants every word typed, in any order and case aside, in each kind's own fields", () => {
    expect(shown(state(), "CART acme-940")).toEqual([["Sessions", ["s:cart"]]]);
    expect(shown(state(), "release sign")).toEqual([["Issues", ["i:ACME-951"]]]);
    expect(shown(state(), "acme-951")).toEqual([["Issues", ["i:ACME-951"]]]);
    expect(shown(state(), "api")).toEqual([
      ["Sessions", ["s:key", "s:perf"]],
      ["Projects", ["p:p-api"]],
    ]);
    expect(shown(state(), "theme light")).toEqual([["Actions", ["a:theme"]]]);
    expect(shown(state(), "pull requests")).toEqual([["Actions", ["a:refresh"]]]);
  });

  it("counts a project's sessions, the orchestrator aside", () => {
    const projects = paletteGroups(state(), "shopfront", CTX).find((g) => g.title === "Projects")!.items;
    expect(projects).toMatchObject([{ kind: "project", sessions: 4 }]);
  });

  it("has no group at all when nothing matches", () => {
    expect(paletteGroups(state(), "zebra quokka", CTX)).toEqual([]);
  });

  it("marks the words typed in what it shows", () => {
    expect(marks("GPS link gps", ["gps"])).toEqual([
      { text: "GPS", hit: true },
      { text: " link ", hit: false },
      { text: "gps", hit: true },
    ]);
    expect(marks("acme-940-cart", ["cart", "940"])).toEqual([
      { text: "acme-", hit: false },
      { text: "940", hit: true },
      { text: "-", hit: false },
      { text: "cart", hit: true },
    ]);
    expect(marks("plain", [])).toEqual([{ text: "plain", hit: false }]);
  });
});

describe("the palette's actions", () => {
  it("are the page's own with no session open, those it cannot do left out", () => {
    expect(actionsOf(CTX)).toEqual([
      "New session",
      "Rank again",
      "Refresh Linear and pull requests",
      "Switch to the light theme",
      "Schedules",
      "Skills",
      "Deck",
    ]);
    expect(actionsOf(CTX, state({ orchestratorId: null }))).not.toContain("Rank again");
  });

  it("follow the TUI and pair phones only where there is a TUI and phones to pair", () => {
    const all = actionsOf({ ...CTX, follow: false, phones: true, theme: "light" });
    expect(all).toContain("Follow TUI");
    expect(all).toContain("Phones");
    expect(all).toContain("Switch to the dark theme");
    expect(actionsOf({ ...CTX, follow: true })).toContain("Stop following TUI");
  });

  it("put the open session's own first: Reply, Terminal, Show in TUI and Mark seen", () => {
    const open = session("cart", { name: "Persist the cart", unseen: true });
    expect(actionsOf({ ...CTX, session: open, follow: false }).slice(0, 5)).toEqual([
      "Reply to Persist the cart",
      "Show the live terminal",
      "Show in TUI",
      "Mark seen",
      "New session",
    ]);
    // No TUI to show it in, a finish already seen, the terminal shown.
    expect(actionsOf({ ...CTX, session: { ...open, unseen: false }, terminal: true }).slice(0, 3)).toEqual([
      "Reply to Persist the cart",
      "Hide the live terminal",
      "New session",
    ]);
  });

  it("name the keys that do the same on the page", () => {
    const items = paletteGroups(state(), "", CTX).find((g) => g.title === "Actions")!.items;
    const keys = Object.fromEntries(items.map((i) => (i.kind === "action" ? [i.action, i.keys] : [])));
    expect(keys).toMatchObject({ new: "N", refresh: "R", rank: null });
  });
});

describe("the palette's keys", () => {
  it("move with the arrows, kept on the list", () => {
    expect(paletteKey("ArrowDown", 0, 3)).toEqual({ kind: "move", to: 1 });
    expect(paletteKey("ArrowDown", 2, 3)).toEqual({ kind: "move", to: 2 });
    expect(paletteKey("ArrowUp", 0, 3)).toEqual({ kind: "move", to: 0 });
    expect(paletteKey("ArrowUp", null, 3)).toEqual({ kind: "move", to: 2 });
    expect(paletteKey("ArrowDown", null, 0)).toEqual({ kind: "move", to: null });
  });

  it("run the lit option on Enter, else the first, and nothing with no results", () => {
    expect(paletteKey("Enter", 2, 3)).toEqual({ kind: "run", at: 2 });
    expect(paletteKey("Enter", null, 3)).toEqual({ kind: "run", at: 0 });
    expect(paletteKey("Enter", null, 0)).toBeNull();
  });

  it("close on Esc, and leave every other key to the field", () => {
    expect(paletteKey("Escape", 1, 3)).toEqual({ kind: "close" });
    expect(paletteKey("a", 1, 3)).toBeNull();
    expect(paletteKey("Tab", 1, 3)).toBeNull();
  });

  type Mods = Partial<{ metaKey: boolean; ctrlKey: boolean; altKey: boolean; shiftKey: boolean }>;
  const k = (key: string, mods: Mods = {}, mac = true) =>
    pageKey({ key, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...mods }, mac);

  it("open on ⌘K on a Mac and Ctrl+K elsewhere, and New on a bare n", () => {
    expect(k("k", { metaKey: true })).toBe("palette");
    expect(k("K", { metaKey: true })).toBe("palette");
    expect(k("k", { ctrlKey: true }, false)).toBe("palette");
    expect(k("k", { metaKey: true, shiftKey: true })).toBeNull();
    expect(k("k")).toBeNull();
    expect(k("n")).toBe("new");
    expect(k("n", { metaKey: true })).toBeNull();
    expect(k("N", { shiftKey: true })).toBeNull();
  });

  it("leave a Mac's Ctrl+K to its text fields, which delete to the end of the line on it", () => {
    expect(k("k", { ctrlKey: true }, true)).toBeNull();
    expect(k("k", { metaKey: true }, false)).toBeNull();
  });
});

describe("the palette's focus", () => {
  it("is given back by focusing what had it, without scrolling to it", () => {
    const opener = { isConnected: true, focus: vi.fn() };
    focusBack(opener);
    expect(opener.focus).toHaveBeenCalledWith({ preventScroll: true });
  });

  it("goes nowhere when that has left the page", () => {
    const gone = { isConnected: false, focus: vi.fn() };
    focusBack(gone);
    expect(gone.focus).not.toHaveBeenCalled();
    expect(() => focusBack(null)).not.toThrow();
  });
});

describe("the palette's dialog", () => {
  const html = () =>
    renderToStaticMarkup(<Palette state={state()} ctx={CTX} now={NOW} phone={false} onPick={() => {}} onClose={() => {}} />);

  it("is a combobox over a listbox, the first option lit", () => {
    const out = html();
    expect(out).toContain('role="combobox"');
    expect(out).toContain('aria-controls="pal-list"');
    expect(out).toContain('aria-expanded="true"');
    expect(out).toContain('aria-activedescendant="pal-opt-0"');
    expect(out).toMatch(/id="pal-list" role="listbox"/);
    expect(out).toMatch(/role="group" aria-labelledby="pal-g-Recent"/);
    expect(out).toMatch(/id="pal-opt-0" role="option" aria-selected="true"/);
    expect(out).toMatch(/id="pal-opt-1" role="option" aria-selected="false"/);
    expect(out).toContain("12 results");
  });

  it("is the whole screen on a phone, with Cancel in place of the keys", () => {
    const out = renderToStaticMarkup(
      <Palette state={state()} ctx={CTX} now={NOW} phone={true} onPick={() => {}} onClose={() => {}} />,
    );
    expect(out).toContain('class="pal sheet"');
    expect(out).toContain(">Cancel</button>");
    expect(out).not.toContain("pal-foot");
  });
});
