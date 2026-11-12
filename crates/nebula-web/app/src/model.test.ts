import { describe, expect, it } from "vitest";
import type { Issue, Session, SessionDetail, State } from "./api/contract";
import {
  ago,
  boardSentence,
  cardAnswers,
  cursorFor,
  followFlash,
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
} from "./model";

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
      { sessionId: "a", reason: "asks", answers: ["yes", "no"] },
      { sessionId: "gone", reason: "x", answers: [] },
      { sessionId: "d", reason: "d", answers: [] },
      { sessionId: "e", reason: "e", answers: [] },
      { sessionId: "b", reason: "b", answers: [] },
    ],
  },
  orchestratorId: null,
  tui: null,
  refresh: { running: false, at: 1, problems: [] },
  today: null,
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
    });
  });

  it("keeps the NEXT line when no pills stand for it", () => {
    const out = splitReply("Body.\nNEXT: a | b", false);
    expect(out).toEqual({ body: "Body.\nNEXT: a | b", report: null });
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
