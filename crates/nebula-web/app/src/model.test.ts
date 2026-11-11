import { describe, expect, it } from "vitest";
import type { Session, SessionDetail, State } from "./api/contract";
import {
  boardSentence,
  cardAnswers,
  cursorFor,
  itemKey,
  keyAction,
  listItems,
  moveCursor,
  parseRoute,
  pickFor,
  routePath,
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
  prUrl: null,
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
  today: null,
  server: { version: "test", protocol: 1 },
  ...over,
});

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
});

describe("the list", () => {
  it("has up to three focus picks, numbered, then the board's other rows in order", () => {
    const items = listItems(state());
    expect(items.map((i) => (i.kind === "focus" ? `${i.n}:${i.sessionId}` : `${i.section}:${i.sessionId}`))).toEqual([
      "1:a",
      "2:d",
      "3:e",
      "running:b",
      "running:c",
    ]);
    expect(items[0]).toMatchObject({ reason: "asks", answers: ["yes", "no"] });
    expect(items.at(-1)).toMatchObject({ captain: true, workers: 2 });
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
    expect(listItems(s).some((i) => i.sessionId === "b")).toBe(false);
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
    expect(cursorFor(items, "focus:a", "a")).toBe("focus:a");
    expect(cursorFor(items, "row:b", "a")).toBe(itemKey(items[0]!));
    expect(cursorFor(items, null, "c")).toBe("row:c");
    expect(cursorFor(items, "row:b", "nowhere")).toBe(null);
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
    expect(boardSentence({ sections: [], idle: 0, stopped: 0 })).toBe("Nothing needs you.");
  });
});
