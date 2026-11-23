import { describe, expect, it } from "vitest";
import type { Issue, Schedule, Session, SessionDetail, State } from "./api/contract";
import {
  actingOn,
  ago,
  cardAnswers,
  cardsShown,
  comingUp,
  composing,
  sendsOnEnter,
  cursorFor,
  dayLine,
  ended,
  focusLine,
  focusPicks,
  followFlash,
  handedLine,
  itemKey,
  itemRoute,
  keyAction,
  keyboardUp,
  look,
  moveCursor,
  needsYouCount,
  NOT_ACTING,
  parseRoute,
  pickFor,
  prStateLabel,
  priorityLabel,
  refreshed,
  routePath,
  rowMeta,
  sectionOf,
  sessionIssue,
  shippedToday,
  showFlash,
  splitReply,
  startOffer,
  unpicked,
  viewItems,
  type ListItem,
} from "./model";
import { NO_SCHEDULES } from "./schedules";

const session = (id: string, over: Partial<Session> = {}): Session => ({
  id,
  name: id,
  projectId: "p",
  projectName: "p",
  branch: "main",
  rootCheckout: false,
  harness: "claude",
  role: "worker",
  status: "finished",
  statusChangedAt: 1,
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

const issue = (key: string, over: Partial<Issue> = {}): Issue => ({
  key,
  url: `https://linear.app/acme/issue/${key}`,
  title: `${key} title`,
  priority: 2,
  state: "In Progress",
  stateType: "started",
  assignee: "Ada",
  branchName: `ada/${key.toLowerCase()}`,
  updatedAt: 1,
  team: "ACME",
  project: null,
  started: null,
  lastProject: null,
  ...over,
});

const state = (over: Partial<State> = {}): State => ({
  projects: [],
  sessions: ["a", "b", "c", "d", "e"].map((id) => session(id)),
  board: {
    sections: [
      { section: "needsYou", rows: [{ sessionId: "a", kind: "session", workers: 0 }] },
      {
        section: "running",
        rows: [
          { sessionId: "b", kind: "session", workers: 0 },
          { sessionId: "c", kind: "captain", workers: 2 },
        ],
      },
    ],
    lanes: [
      { lane: "inFlight", issues: [issue("ACME-1")] },
      { lane: "todo", issues: [issue("ACME-2", { stateType: "unstarted" }), issue("ACME-3", { stateType: "unstarted" })] },
    ],
    idle: 0,
    stopped: 0,
  },
  focus: {
    source: "rule",
    at: null,
    picks: [
      { key: "a", sessionId: "a", reason: "asks", answers: ["yes", "no"] },
      { key: "gone", sessionId: "gone", reason: "x", answers: [] },
      { key: "d", sessionId: "d", reason: "d", answers: [] },
      { key: "e", sessionId: "e", reason: "e", answers: [] },
      { key: "b", sessionId: "b", reason: "b", answers: [] },
    ],
    fallback: null,
    rank: null,
  },
  orchestratorId: null,
  tui: null,
  refresh: { running: false, at: 1, problems: [] },
  today: null,
  todayProblem: null,
  schedules: NO_SCHEDULES,
  server: { version: "test", protocol: 1 },
  ...over,
});

/// An item as `running:b`, `todo:ACME-2` or `found:c`.
const said = (i: ListItem) => (i.kind === "issue" ? `${i.lane}:${i.issue.key}` : `${i.group}:${i.sessionId}`);

const detail = (over: Partial<SessionDetail> = {}): SessionDetail => ({
  session: session("a"),
  reply: null,
  answers: ["yes", "no"],
  dialog: null,
  workerIds: [],
  ...over,
});

const key = (k: string, mods: Partial<{ metaKey: boolean; ctrlKey: boolean; altKey: boolean }> = {}) =>
  keyAction({ key: k, metaKey: false, ctrlKey: false, altKey: false, ...mods });

describe("the on-screen keyboard", () => {
  it("is up when the visual viewport is far shorter than the layout viewport, as in an iOS Home Screen app", () => {
    // An iPhone's Home Screen app: innerHeight is 394 too.
    expect(keyboardUp({ scale: 1, height: 394 }, 797)).toBe(true);
    expect(keyboardUp({ scale: 1, height: 797 }, 797)).toBe(false);
    // Safari's toolbar coming and going, or a pinch zoom, is no keyboard.
    expect(keyboardUp({ scale: 1, height: 740 }, 797)).toBe(false);
    expect(keyboardUp({ scale: 2, height: 398 }, 797)).toBe(false);
  });
});

describe("routes", () => {
  it("parses idle, session and anything else", () => {
    expect(parseRoute("/")).toEqual({ kind: "idle" });
    expect(parseRoute("/s/abc")).toEqual({ kind: "session", id: "abc" });
    expect(parseRoute("/s/abc/")).toEqual({ kind: "session", id: "abc" });
    expect(parseRoute("/s/a%2Fb")).toEqual({ kind: "session", id: "a/b" });
    expect(parseRoute("/s/")).toEqual({ kind: "idle" });
    expect(parseRoute("/s/a/b")).toEqual({ kind: "idle" });
    expect(parseRoute("/s/%E0%A4%A")).toEqual({ kind: "idle" });
    expect(parseRoute("/nope")).toEqual({ kind: "idle" });
  });

  it("round-trips a session id", () => {
    const r = { kind: "session", id: "a b/c" } as const;
    expect(parseRoute(routePath(r))).toEqual(r);
    expect(routePath({ kind: "idle" })).toBe("/");
  });

  it("has a path of its own for the orchestrator's thread, whichever session it is", () => {
    expect(parseRoute("/orchestrator")).toEqual({ kind: "orchestrator" });
    expect(parseRoute("/orchestrator/")).toEqual({ kind: "orchestrator" });
    expect(routePath({ kind: "orchestrator" })).toBe("/orchestrator");
    expect(sectionOf({ kind: "orchestrator" })).toBe("attention");
  });

  it("opens an issue by its key", () => {
    expect(parseRoute("/i/ACME-12")).toEqual({ kind: "issue", key: "ACME-12" });
    expect(routePath({ kind: "issue", key: "ACME-12" })).toBe("/i/ACME-12");
    expect(parseRoute("/i/")).toEqual({ kind: "idle" });
  });
});

describe("the list", () => {
  it("has up to three focus picks of the sessions and issues the state has, numbered", () => {
    const picks = focusPicks(state());
    expect(picks.map((p) => `${p.n}:${p.sessionId}`)).toEqual(["1:a", "2:d", "3:e"]);
    expect(picks[0]).toMatchObject({ reason: "asks", answers: ["yes", "no"] });
  });

  it("has the board's rows in the Inbox, the picked ones too, section by section", () => {
    const items = viewItems(state(), "inbox", null);
    expect(items.map(said)).toEqual(["needsYou:a", "running:b", "running:c"]);
    expect(items[2]).toMatchObject({ captain: true, workers: 2 });
    expect(itemKey(items[0]!)).toBe("row:a");
    expect(itemRoute(items[0]!)).toEqual({ kind: "session", id: "a" });
  });

  it("has every session at work under Running, a captain's workers too, and every lane under Issues", () => {
    const s = state();
    s.sessions = s.sessions.map((x) => (["b", "d"].includes(x.id) ? { ...x, status: "running" } : x));
    expect(viewItems(s, "running", null).map(said)).toEqual(["running:b", "running:d"]);
    const issues = viewItems(s, "issues", null);
    expect(issues.map(said)).toEqual(["inFlight:ACME-1", "todo:ACME-2", "todo:ACME-3"]);
    expect(itemKey(issues.at(-1)!)).toBe("issue:ACME-3");
    expect(itemRoute(issues.at(-1)!)).toEqual({ kind: "issue", key: "ACME-3" });
    expect(viewItems(s, "today", null)).toEqual([]);
  });

  it("has every session by its project under Projects, in the daemon's order of projects", () => {
    const s = state({
      projects: [
        { id: "q", name: "q", space: null, team: null },
        { id: "p", name: "p", space: null, team: "ACME" },
      ],
    });
    s.sessions = s.sessions.map((x) => (x.id === "d" ? { ...x, projectId: "q", projectName: "q" } : x));
    expect(viewItems(s, "projects", null).map(said)).toEqual(["q:d", "p:a", "p:b", "p:c", "p:e"]);
  });

  it("shows one project's rows when one is picked, and the issues of the team it reads", () => {
    const s = state({ projects: [{ id: "p", name: "p", space: null, team: "ACME" }, { id: "q", name: "q", space: null, team: "DOC" }] });
    s.sessions = s.sessions.map((x) => (x.id === "b" ? { ...x, projectId: "q", projectName: "q" } : x));
    expect(viewItems(s, "inbox", "q").map(said)).toEqual(["running:b"]);
    expect(viewItems(s, "issues", "p")).toHaveLength(3);
    expect(viewItems(s, "issues", "q")).toEqual([]);
  });

  it("leaves out rows of sessions it does not know", () => {
    const s = state();
    s.sessions = s.sessions.filter((x) => x.id !== "b");
    expect(viewItems(s, "inbox", null).some((i) => i.kind === "row" && i.sessionId === "b")).toBe(false);
  });

  it("counts what needs you, and keeps in sight what the picks leave out of it", () => {
    const s = state();
    expect(needsYouCount(s)).toBe(1);
    expect(unpicked(s, focusPicks(s))).toEqual([]);
    s.focus.picks = s.focus.picks.filter((p) => p.sessionId !== "a");
    expect(unpicked(s, focusPicks(s)).map((i) => i.sessionId)).toEqual(["a"]);
  });

  it("moves the cursor within the list", () => {
    expect(moveCursor(null, 1, 4)).toBe(0);
    expect(moveCursor(null, -1, 4)).toBe(3);
    expect(moveCursor(0, -1, 4)).toBe(0);
    expect(moveCursor(3, 1, 4)).toBe(3);
    expect(moveCursor(1, 1, 4)).toBe(2);
    expect(moveCursor(2, 1, 0)).toBe(null);
  });

  it("keeps the cursor on the opened session's row, else takes its first", () => {
    const items = viewItems(state(), "inbox", null);
    const open = (id: string) => ({ kind: "session", id }) as const;
    expect(cursorFor(items, "row:a", open("a"))).toBe("row:a");
    expect(cursorFor(items, "row:b", open("a"))).toBe("row:a");
    expect(cursorFor(items, null, open("c"))).toBe("row:c");
    expect(cursorFor(items, "row:b", open("nowhere"))).toBe(null);
  });

  it("puts the cursor on an opened issue's row, never on a session's", () => {
    const items = viewItems(state(), "issues", null);
    expect(cursorFor(items, "row:b", { kind: "issue", key: "ACME-2" })).toBe("issue:ACME-2");
    expect(cursorFor(items, "issue:ACME-2", { kind: "issue", key: "ACME-2" })).toBe("issue:ACME-2");
    expect(cursorFor(items, "issue:ACME-2", { kind: "issue", key: "ACME-9" })).toBe(null);
    expect(cursorFor(items, "issue:ACME-2", { kind: "session", id: "ACME-2" })).toBe(null);
  });

  it("names a session's issue by the Linear issue it works on, else by its issue URL", () => {
    expect(sessionIssue(session("a"))).toBe(null);
    const url = { issueUrl: "https://github.com/o/r/issues/3", issueKey: "#3" };
    expect(sessionIssue(session("a", url))).toEqual({ key: "#3", url: url.issueUrl });
    expect(sessionIssue(session("a", { ...url, issue: issue("ACME-7") }))).toEqual({
      key: "ACME-7",
      url: "https://linear.app/acme/issue/ACME-7",
    });
  });
});

describe("refresh", () => {
  it("says when the issues and pull requests were read, in words kept current", () => {
    const at = 10 * 86_400_000;
    const r = (over: Partial<State["refresh"]>) => ({ running: false, at, problems: [], ...over });
    expect(refreshed(r({}), at + 30_000)).toBe("Updated just now");
    expect(refreshed(r({}), at + 4 * 60_000)).toBe("Updated 4 min ago");
    expect(refreshed(r({}), at + 61 * 60_000)).toBe("Updated 1 hour ago");
    expect(refreshed(r({ running: true }), at)).toBe("Refreshing…");
    expect(refreshed(r({ at: null }), at)).toBe("Not read yet");
    expect(ago(at, at + 3 * 3_600_000)).toBe("3 hours ago");
    expect(ago(at, at + 2 * 86_400_000)).toBe("2 days ago");
    expect(ago(at, at - 5000)).toBe("just now");
  });

  it("names Linear's priorities and a pull request's states in the TUI's words", () => {
    expect([0, 1, 2, 3, 4, 7].map(priorityLabel)).toEqual(["No priority", "Urgent", "High", "Medium", "Low", "No priority"]);
    expect(prStateLabel("open")).toBe("ready for review");
    expect(prStateLabel("open", true)).toBe("ready");
    expect(prStateLabel("draft", true)).toBe("draft");
    expect(prStateLabel("merged")).toBe("merged");
  });
});

describe("keys", () => {
  it("maps keys to actions", () => {
    expect(key("j")).toEqual({ kind: "move", delta: 1 });
    expect(key("ArrowDown")).toEqual({ kind: "move", delta: 1 });
    expect(key("k")).toEqual({ kind: "move", delta: -1 });
    expect(key("ArrowUp")).toEqual({ kind: "move", delta: -1 });
    expect(key("Enter")).toEqual({ kind: "open" });
    expect(key("Escape")).toEqual({ kind: "back" });
    expect(key("t")).toEqual({ kind: "terminal" });
    expect(key("r")).toEqual({ kind: "refresh" });
    expect(key("s")).toEqual({ kind: "start" });
    expect(key("3")).toEqual({ kind: "pick", n: 3 });
    expect(key("0")).toBe(null);
    expect(key("x")).toBe(null);
  });

  it("leaves the browser's shortcuts alone", () => {
    expect(key("t", { metaKey: true })).toBe(null);
    expect(key("1", { ctrlKey: true })).toBe(null);
    expect(key("j", { altKey: true })).toBe(null);
  });

  it("picks an answer, or with a dialog open that dialog's option", () => {
    expect(pickFor(2, detail())).toEqual({ kind: "answer", text: "no" });
    expect(pickFor(3, detail())).toBe(null);
    const dialog = { question: "Go?", options: [{ number: 1, label: "Yes" }, { number: 2, label: "No" }] };
    expect(pickFor(2, detail({ dialog }))).toEqual({ kind: "option", option: 2 });
    expect(pickFor(3, detail({ dialog }))).toBe(null);
  });

  it("offers a focus card's answers only while its session takes one", () => {
    const card = focusPicks(state())[0]!;
    expect(cardAnswers(card, session("a"))).toEqual(["yes", "no"]);
    for (const status of ["running", "needsFeedback", "terminated", "disconnected"] as const) {
      expect(cardAnswers(card, session("a", { status }))).toEqual([]);
    }
  });

  it("picks no answer while the session runs or has stopped", () => {
    expect(pickFor(1, detail({ session: session("a", { status: "running" }) }))).toBe(null);
    expect(pickFor(1, detail({ session: session("a", { status: "terminated" }) }))).toBe(null);
  });

  it("answer a focus card only while the cards are on screen, never from an issue's page", () => {
    expect(cardsShown({ kind: "idle" }, false, "inbox", false)).toBe(true);
    expect(cardsShown({ kind: "idle" }, false, "running", true)).toBe(true);
    expect(cardsShown({ kind: "issue", key: "ACME-1" }, false, "inbox", false)).toBe(false);
    expect(cardsShown({ kind: "session", id: "a" }, false, "inbox", false)).toBe(false);
    expect(cardsShown({ kind: "new" }, false, "inbox", false)).toBe(false);
    // Narrower than Home, the cards head the inbox alone, and a find hides them.
    expect(cardsShown({ kind: "idle" }, true, "inbox", false)).toBe(true);
    expect(cardsShown({ kind: "idle" }, true, "running", false)).toBe(false);
    expect(cardsShown({ kind: "idle" }, true, "inbox", true)).toBe(false);
  });

  it("leave every key to an IME while it composes, Safari's confirming Enter too", () => {
    expect(composing({ isComposing: true, keyCode: 13 })).toBe(true);
    expect(composing({ isComposing: false, keyCode: 229 })).toBe(true);
    expect(composing({ isComposing: false, keyCode: 13 })).toBe(false);
  });

  it("send on Enter with a keyboard and add a line on Shift+Enter or a touch screen's Return", () => {
    const enter = (mods: Partial<Parameters<typeof sendsOnEnter>[0]> = {}) => ({
      key: "Enter", shiftKey: false, altKey: false, metaKey: false, ctrlKey: false, isComposing: false, keyCode: 13, ...mods,
    });
    expect(sendsOnEnter(enter(), false)).toBe(true);
    expect(sendsOnEnter(enter({ shiftKey: true }), false)).toBe(false);
    expect(sendsOnEnter(enter({ altKey: true }), false)).toBe(false);
    expect(sendsOnEnter(enter(), true)).toBe(false);
    expect(sendsOnEnter(enter({ metaKey: true }), true)).toBe(true);
    expect(sendsOnEnter(enter({ ctrlKey: true }), false)).toBe(true);
    expect(sendsOnEnter(enter({ keyCode: 229 }), false)).toBe(false);
    expect(sendsOnEnter(enter({ key: "a" }), false)).toBe(false);
  });
});

describe("what the page asked", () => {
  const sent = (on: string, seq: number) => ({ on, seq, busy: true, error: null });

  it("is shown on its own session's page alone", () => {
    const a = sent("A", 1);
    expect(actingOn(a, "A")).toEqual({ busy: true, error: null });
    expect(actingOn(a, "B")).toEqual({ busy: false, error: null });
    const failed = ended(a, 1, "The session is running.");
    expect(actingOn(failed, "A")).toEqual({ busy: false, error: "The session is running." });
    expect(actingOn(failed, "B")).toEqual({ busy: false, error: null });
    expect(actingOn(NOT_ACTING, "A")).toEqual({ busy: false, error: null });
  });

  it("is the last ask: an earlier one answering late changes nothing", () => {
    // Sent to A, then to B before A answered.
    const b = sent("B", 2);
    const late = ended(b, 1, "A failed.");
    expect(late).toBe(b);
    expect(actingOn(late, "B")).toEqual({ busy: true, error: null });
    expect(actingOn(late, "A")).toEqual({ busy: false, error: null });
    expect(actingOn(ended(late, 2, null), "B")).toEqual({ busy: false, error: null });
  });
});

describe("replies", () => {
  it("splits off the report line and the NEXT line its pills stand for", () => {
    const text = "Body.\n\nQUESTION: keep it, or not?\nNEXT: keep | drop\n";
    expect(splitReply(text, true)).toEqual({
      body: "Body.",
      report: { word: "QUESTION", text: "keep it, or not?" },
      next: null,
    });
  });

  it("keeps the NEXT line apart, after the report, when no pills stand for it", () => {
    expect(splitReply("Body.\nNEXT: a | b", false)).toEqual({ body: "Body.", report: null, next: "NEXT: a | b" });
    expect(splitReply("Body.\n\nBLOCKED: no key\nNEXT: add it | skip", false)).toEqual({
      body: "Body.",
      report: { word: "BLOCKED", text: "no key" },
      next: "NEXT: add it | skip",
    });
  });

  it("reads a bold report word and drops answers on the same line", () => {
    expect(splitReply("**BLOCKED:** no key NEXT: add it", false).report).toEqual({
      word: "BLOCKED",
      text: "no key",
    });
    expect(splitReply("DONEISH is not a report", false).report).toBe(null);
  });
});

describe("the day", () => {
  it("says what the board holds under the date", () => {
    expect(dayLine(state().board)).toBe("1 needs you · 2 running");
    expect(dayLine({ sections: [], lanes: [], idle: 0, stopped: 0 })).toBe("No session is running.");
  });

  it("says who picked the focus, and when", () => {
    const at = new Date(2026, 10, 19, 7, 30).getTime();
    const by = (source: "rule" | "orchestrator") => state({ focus: { ...state().focus, source, at } });
    expect(focusLine(by("orchestrator"), 3)).toBe("The orchestrator picked three at 07:30.");
    expect(focusLine(by("orchestrator"), 0)).toBe("The orchestrator picked nothing at 07:30.");
    expect(focusLine(by("rule"), 1)).toBe("By rule: the board's first row that needs you or is done.");
    expect(focusLine(by("rule"), 2)).toBe("By rule: the board's first two rows that need you or are done.");
    expect(focusLine(by("rule"), 0)).toBe("");
  });

  it("ships the workers that finished today, newest first, and none that ask or are blocked", () => {
    const now = new Date(2026, 10, 19, 16).getTime();
    const s = state({
      sessions: [
        session("old", { statusChangedAt: now - 30 * 3_600_000 }),
        session("first", { statusChangedAt: now - 5 * 3_600_000 }),
        session("last", { statusChangedAt: now - 60_000, prUrl: "https://github.com/o/r/pull/9", prState: "merged" }),
        session("asks", { statusChangedAt: now - 60_000, reportWord: "QUESTION" }),
        session("cap", { statusChangedAt: now - 60_000, role: "captain" }),
        session("busy", { statusChangedAt: now - 60_000, status: "running" }),
      ],
    });
    expect(shippedToday(s, now).map((x) => x.id)).toEqual(["last", "first"]);
  });

  it("names the schedules that run next, soonest first, and none that are done", () => {
    const sched = (id: string, nextAt: number | null): Schedule => ({
      id,
      name: id,
      task: "rank",
      projectId: null,
      projectName: null,
      prompt: null,
      harness: null,
      model: null,
      effort: null,
      permissionMode: null,
      target: null,
      when: { kind: "every", minutes: 60 },
      nextAt,
      createdAt: 0,
      sessionId: null,
    });
    const schedules = { ...state().schedules, schedules: [sched("late", 30), sched("done", null), sched("soon", 10)] };
    expect(comingUp(schedules).map((x) => x.id)).toEqual(["soon", "late"]);
  });
});

describe("a row", () => {
  it("says what a session asks in its pill, and its pull request's state otherwise", () => {
    expect(look(session("a", { status: "needsFeedback" }))).toMatchObject({ tone: "peach", pill: { text: "Dialog" } });
    expect(look(session("a", { reportWord: "QUESTION" }))).toMatchObject({ tone: "red", pill: { text: "Question", tint: "red" } });
    expect(look(session("a", { reportWord: "BLOCKED" }))).toMatchObject({ tone: "red", pill: { text: "Blocked" } });
    const pr = { prUrl: "https://github.com/o/r/pull/57", prState: "open" } as const;
    expect(look(session("a", { unseen: true, ...pr }))).toMatchObject({ label: "Done", tone: "blue", pill: { text: "PR #57 ready", tint: "green" } });
    expect(look(session("a", { status: "running" }))).toMatchObject({ tone: "green", pulse: true, pill: null });
    expect(look(session("a", { role: "captain" }))).toMatchObject({ label: "At rest", tone: "hollow" });
  });

  it("says where a session is under its name: project, the issue its branch does not name, branch, and what it says", () => {
    const linked = { issue: issue("ACME-7") };
    expect(rowMeta(session("a", { projectName: "api", branch: "acme-7-fix", ...linked }), 0)).toBe("api · acme-7-fix");
    expect(rowMeta(session("a", { projectName: "api", branch: "main", ...linked }), 0)).toBe("api · ACME-7 · main");
    expect(rowMeta(session("a", { name: "main", projectName: "api" }), 0)).toBe("api");
    expect(rowMeta(session("a", { projectName: "api", why: "QUESTION: keep it?" }), 0)).toBe("api · main · keep it?");
    expect(rowMeta(session("cap", { name: "captain", role: "captain", projectName: "api" }), 2)).toBe("api · 2 workers");
    expect(rowMeta(session("cap", { name: "Rover", role: "captain", projectName: "api" }), 1)).toBe("api · captain · 1 worker");
    expect(rowMeta(session("a", { projectName: "api", branch: "fix" }), 0, false)).toBe("fix");
  });
});

describe("the TUI", () => {
  it("says whether a show put the session up or found it up, and a refusal as the server said it", () => {
    expect(showFlash({ alreadyOnScreen: false })).toEqual({ text: "Shown in the TUI.", error: false });
    expect(showFlash({ alreadyOnScreen: true })).toEqual({ text: "Already on screen in the TUI.", error: false });
    expect(showFlash({ error: "No TUI is open to show the session in." })).toEqual({
      text: "No TUI is open to show the session in.",
      error: true,
    });
  });

  it("says Follow TUI changed nothing only when the page is where the TUI is", () => {
    const on = (sessionId: string | null) => ({ sessionId });
    expect(followFlash({ kind: "session", id: "a" }, on("a"))).toEqual({
      text: "Already on the session the TUI shows.",
      error: false,
    });
    expect(followFlash({ kind: "idle" }, on(null))).toEqual({ text: "The TUI shows no session.", error: false });
    expect(followFlash({ kind: "session", id: "a" }, on("b"))).toBe(null);
    expect(followFlash({ kind: "idle" }, on("a"))).toBe(null);
    expect(followFlash({ kind: "session", id: "a" }, on(null))).toBe(null);
  });
});

describe("Start", () => {
  const projects = [
    { id: "p1", name: "api", space: "Acme", team: "ACME" },
    { id: "p2", name: "web", space: "Acme", team: "ACME" },
    { id: "p3", name: "docs", space: null, team: "DOC" },
    { id: "p4", name: "tools", space: null, team: null },
  ];
  const names = (o: ReturnType<typeof startOffer>) => [o.projects.map((p) => p.name), o.pick];

  it("offers every project of the issue's team, the one last used first", () => {
    expect(names(startOffer(projects, issue("ACME-1")))).toEqual([["api", "web"], "p1"]);
    expect(names(startOffer(projects, issue("ACME-1", { lastProject: "p2" })))).toEqual([["api", "web"], "p2"]);
    expect(names(startOffer(projects, issue("ACME-1", { lastProject: "p3" })))).toEqual([["api", "web"], "p1"]);
    expect(names(startOffer(projects, issue("DOC-1", { team: "DOC" })))).toEqual([["docs"], "p3"]);
    expect(names(startOffer(projects, issue("OPS-1", { team: "OPS" })))).toEqual([[], null]);
  });

  it("says where it went, when, and whether its brief waits", () => {
    const at = new Date(2026, 10, 13, 14, 2).getTime();
    const started = { projectId: "p1", projectName: "api", captainId: "c", at, held: false, moveFailed: null };
    expect(handedLine(started, at)).toBe(
      "Handed to the captain of api at 14:02. The page goes to its worker once the captain starts it.",
    );
    expect(handedLine({ ...started, held: true }, at)).toBe(
      "Handed to the captain of api at 14:02. The brief waits until the captain is at rest with an empty input box.",
    );
  });
});
