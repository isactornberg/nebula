import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Session, SessionDetail, State } from "./api/contract";
import { deckGrid, deckIds, holdOrder, offered, press } from "./deck";
import { pickFor } from "./model";

const session = (id: string, over: Partial<Session> = {}) => ({ id, role: "worker", status: "finished", ...over }) as Session;

describe("the Deck's tiles", () => {
  it("are the board's rows, then the sessions it leaves off in the state's order, the orchestrator never", () => {
    const state = {
      sessions: [
        session("run", { status: "running" }),
        session("idle"),
        session("asks", { reportWord: "QUESTION" }),
        session("cap", { role: "captain" }),
        session("orch", { role: "orchestrator" }),
        session("folded"),
      ],
      orchestratorId: "orch",
      board: {
        sections: [
          { section: "needsYou", rows: [{ sessionId: "asks", kind: "session", workers: 0 }] },
          { section: "running", rows: [{ sessionId: "run", kind: "session", workers: 0 }] },
          { section: "captains", rows: [{ sessionId: "cap", kind: "captain", workers: 0 }, { sessionId: "gone", kind: "session", workers: 0 }] },
        ],
      },
    } as unknown as State;
    expect(deckIds(state)).toEqual(["asks", "run", "cap", "idle", "folded"]);
  });

  it("keep their places while they hold still, a gone one's place empty and a new one last", () => {
    expect(holdOrder(["a", "b", "c"], ["c", "a", "b"])).toEqual(["a", "b", "c"]);
    expect(holdOrder(["a", "b", "c"], ["d", "c", "a"])).toEqual(["a", null, "c", "d"]);
    expect(holdOrder(["a", null, "c", "d"], ["e", "d", "a"])).toEqual(["a", null, null, "d", "e"]);
  });
});

describe("the Deck's grid", () => {
  const grid = (w: number, h: number) => {
    const g = deckGrid(w, h);
    return `${g.cols}x${g.rows}`;
  };

  it("is eight tiles on a phone, on its side or upright", () => {
    expect(grid(844, 390)).toBe("4x2");
    expect(grid(390, 844)).toBe("2x4");
    expect(grid(667, 375)).toBe("4x2");
  });

  it("grows to 12 to 16 on a tablet and a larger screen", () => {
    expect(grid(1280, 800)).toBe("4x3");
    expect(grid(800, 1280)).toBe("3x5");
    expect(grid(1024, 768)).toBe("4x4");
    expect(grid(1440, 1000)).toBe("4x4");
  });
});

describe("a press on a tile", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  const spies = () => {
    const tap = vi.fn();
    const hold = vi.fn();
    return { tap, hold, p: press(tap, hold, 500) };
  };

  it("let go early is a tap alone", () => {
    const { tap, hold, p } = spies();
    p.down();
    vi.advanceTimersByTime(499);
    p.up();
    vi.advanceTimersByTime(1000);
    expect([tap.mock.calls.length, hold.mock.calls.length]).toEqual([1, 0]);
  });

  it("held for half a second is a hold alone, and letting go then taps nothing", () => {
    const { tap, hold, p } = spies();
    p.down();
    vi.advanceTimersByTime(500);
    p.up();
    expect([tap.mock.calls.length, hold.mock.calls.length]).toEqual([0, 1]);
  });

  it("that leaves the tile is neither", () => {
    const { tap, hold, p } = spies();
    p.down();
    p.cancel();
    vi.advanceTimersByTime(1000);
    p.up();
    expect([tap.mock.calls.length, hold.mock.calls.length]).toEqual([0, 0]);
  });
});

describe("the bar's answers", () => {
  const detail = (over: Partial<SessionDetail>, s: Partial<Session> = {}) =>
    ({ session: session("x", s), reply: null, answers: [], dialog: null, workerIds: [], ...over }) as SessionDetail;

  it("are an open dialog's options, picked by their numbers", () => {
    const d = detail({
      answers: ["Merge it"],
      dialog: { question: "Run it?", options: [{ number: 1, label: "Yes" }, { number: 2, label: "No" }] },
    }, { status: "needsFeedback" });
    expect(offered(d)).toEqual([{ n: 1, label: "Yes" }, { n: 2, label: "No" }]);
    expect(pickFor(2, d)).toEqual({ kind: "option", option: 2 });
  });

  it("are the reply's answers in order, sent as text", () => {
    const d = detail({ answers: ["Merge it", "Wait"] });
    expect(offered(d)).toEqual([{ n: 1, label: "Merge it" }, { n: 2, label: "Wait" }]);
    expect(pickFor(1, d)).toEqual({ kind: "answer", text: "Merge it" });
  });

  it("are none while the session runs", () => {
    expect(offered(detail({ answers: ["Merge it"] }, { status: "running" }))).toEqual([]);
  });
});
