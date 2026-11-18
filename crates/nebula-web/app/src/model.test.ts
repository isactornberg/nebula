import { describe, expect, it } from "vitest";
import type { Issue, Session, SessionDetail, State } from "./api/contract";
import {
  ago,
  boardSentence,
  cardAnswers,
  cursorFor,
  followFlash,
  handedLine,
  itemKey,
  itemRoute,
  keyAction,
  listItems,
  moveCursor,
  parseRoute,
  pickFor,
  prStateLabel,
  priorityLabel,
  refreshed,
  routePath,
  sessionIssue,
  showFlash,
  splitReply,
  startOffer,
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

/// An item as `1:a`, `running:b` or `todo:ACME-2`.
const said = (i: ReturnType<typeof listItems>[number]) =>
  i.kind === "focus" ? `${i.n}:${i.sessionId}` : i.kind === "issue" ? `${i.lane}:${i.issue.key}` : `${i.section}:${i.sessionId}`;

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

  it("opens an issue by its key", () => {
    expect(parseRoute("/i/ACME-12")).toEqual({ kind: "issue", key: "ACME-12" });
    expect(routePath({ kind: "issue", key: "ACME-12" })).toBe("/i/ACME-12");
    expect(parseRoute("/i/")).toEqual({ kind: "idle" });
  });
});

describe("the list", () => {
  it("has up to three focus picks, numbered, then the board's other rows in order, then the issue lanes", () => {
    const items = listItems(state());
    expect(items.map(said)).toEqual([
      "1:a",
      "2:d",
      "3:e",
      "running:b",
      "running:c",
      "inFlight:ACME-1",
      "todo:ACME-2",
      "todo:ACME-3",
    ]);
    expect(items[0]).toMatchObject({ reason: "asks", answers: ["yes", "no"] });
    expect(items[4]).toMatchObject({ captain: true, workers: 2 });
    expect(itemKey(items.at(-1)!)).toBe("issue:ACME-3");
    expect(itemRoute(items.at(-1)!)).toEqual({ kind: "issue", key: "ACME-3" });
  });

  it("puts a session back in its lane once it is no pick shown", () => {
    const s = state();
    s.focus.picks = s.focus.picks.filter((p) => p.sessionId !== "a");
    const items = listItems(s);
    expect(items.filter((i) => i.kind === "focus").map((i) => i.sessionId)).toEqual(["d", "e", "b"]);
    expect(items.filter((i) => i.kind === "row").map((i) => i.sessionId)).toEqual(["a", "c"]);
  });

  it("leaves out rows of sessions it does not know", () => {
    const s = state();
    s.sessions = s.sessions.filter((x) => x.id !== "b");
    expect(listItems(s).some((i) => i.kind !== "issue" && i.sessionId === "b")).toBe(false);
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
    const items = listItems(state());
    const open = (id: string) => ({ kind: "session", id }) as const;
    expect(cursorFor(items, "focus:a", open("a"))).toBe("focus:a");
    expect(cursorFor(items, "row:b", open("a"))).toBe(itemKey(items[0]!));
    expect(cursorFor(items, null, open("c"))).toBe("row:c");
    expect(cursorFor(items, "row:b", open("nowhere"))).toBe(null);
  });

  it("puts the cursor on an opened issue's row, never on a session's", () => {
    const items = listItems(state());
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
    const card = listItems(state())[0]!;
    if (card.kind !== "focus") throw new Error("a focus item first");
    expect(cardAnswers(card, session("a"))).toEqual(["yes", "no"]);
    for (const status of ["running", "needsFeedback", "terminated", "disconnected"] as const) {
      expect(cardAnswers(card, session("a", { status }))).toEqual([]);
    }
  });

  it("picks no answer while the session runs or has stopped", () => {
    expect(pickFor(1, detail({ session: session("a", { status: "running" }) }))).toBe(null);
    expect(pickFor(1, detail({ session: session("a", { status: "terminated" }) }))).toBe(null);
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

describe("summary", () => {
  it("says what the board holds", () => {
    expect(boardSentence(state().board)).toBe("One session needs you and two are running.");
    expect(boardSentence({ sections: [], lanes: [], idle: 0, stopped: 0 })).toBe("No session is running.");
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
