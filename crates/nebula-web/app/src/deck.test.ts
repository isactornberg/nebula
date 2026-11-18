import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Session, SessionDetail, State } from "./api/contract";
import { deckGrid, deckIds, HOLD_MS, holdOrder, offered, press, SLOP_PX, SWIPE_PX } from "./deck";
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

describe("a touch on the tiles", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  /// What a touch was: each tap, hold, swipe and change of armed, in order.
  const touch = () => {
    const said: string[] = [];
    const p = press({
      tap: () => said.push("tap"),
      hold: () => said.push("hold"),
      swipe: (by) => said.push(`swipe ${by}`),
      armed: (on) => said.push(on ? "armed" : "unarmed"),
    });
    return { said, p };
  };
  /// A finger down at 100,100, moved to each point in turn, then let go
  /// where it was last.
  const finger = (p: ReturnType<typeof press>, path: [number, number, number][]) => {
    p.down(100, 100);
    let at: [number, number] = [100, 100];
    for (const [ms, x, y] of path) {
      vi.advanceTimersByTime(ms);
      at = [x, y];
      p.move(x, y);
    }
    p.up(...at);
  };

  it("let go early is a tap alone", () => {
    const { said, p } = touch();
    finger(p, [[499, 100, 100]]);
    vi.advanceTimersByTime(1000);
    expect(said).toEqual(["tap"]);
  });

  it("held for half a second is armed at once, and a hold alone once let go", () => {
    const { said, p } = touch();
    p.down(100, 100);
    vi.advanceTimersByTime(HOLD_MS);
    expect(said).toEqual(["armed"]);
    vi.advanceTimersByTime(2000);
    p.up(100, 100);
    expect(said).toEqual(["armed", "unarmed", "hold"]);
  });

  it("drifting within the slop is still a tap or a hold", () => {
    const tap = touch();
    finger(tap.p, [[100, 106, 104], [100, 100 - SLOP_PX, 100]]);
    expect(tap.said).toEqual(["tap"]);
    const hold = touch();
    finger(hold.p, [[300, 107, 103], [400, 103, 107]]);
    expect(hold.said).toEqual(["armed", "unarmed", "hold"]);
  });

  it("is a swipe once it goes sideways past the slop and is let go far enough to one side", () => {
    const left = touch();
    finger(left.p, [[30, 88, 101], [30, 60, 104], [30, 100 - SWIPE_PX, 112]]);
    expect(left.said).toEqual(["swipe 1"]);
    const right = touch();
    finger(right.p, [[60, 112, 98], [60, 250, 90]]);
    expect(right.said).toEqual(["swipe -1"]);
    // Let go where the last move was not: the release's own position counts.
    const flick = touch();
    flick.p.down(100, 100);
    vi.advanceTimersByTime(80);
    flick.p.up(30, 100);
    expect(flick.said).toEqual(["swipe 1"]);
  });

  it("gone sideways and let go short of a swipe, or more up and down by then, is nothing", () => {
    const short = touch();
    finger(short.p, [[50, 100 - SLOP_PX - 1, 100], [50, 100 - SWIPE_PX + 1, 100]]);
    const turned = touch();
    finger(turned.p, [[50, 85, 100], [50, 40, 170]]);
    vi.advanceTimersByTime(1000);
    expect([short.said, turned.said]).toEqual([[], []]);
  });

  it("is never a tap or a hold once it strays, and never a swipe once it is held", () => {
    // Up or down first: the browser's own to pan, and nothing of ours.
    const down = touch();
    finger(down.p, [[50, 102, 100 + SLOP_PX + 1], [50, 30, 105]]);
    // A hold that moves away is no hold, and no swipe however far it goes.
    const held = touch();
    finger(held.p, [[HOLD_MS, 104, 100], [100, 100 + SLOP_PX + 1, 100], [100, 300, 100]]);
    // Sideways a moment before it would have been held: a swipe, not a hold.
    const late = touch();
    finger(late.p, [[HOLD_MS - 1, 115, 100], [100, 20, 100]]);
    expect([down.said, held.said, late.said]).toEqual([[], ["armed", "unarmed"], ["swipe 1"]]);
  });

  it("cancelled is none of them, held long or not, and the next touch starts afresh", () => {
    const { said, p } = touch();
    p.down(100, 100);
    p.cancel();
    vi.advanceTimersByTime(1000);
    p.up(100, 100);
    p.move(10, 100);
    p.down(100, 100);
    vi.advanceTimersByTime(600);
    p.cancel();
    p.up(100, 100);
    expect(said).toEqual(["armed", "unarmed"]);
    finger(p, [[100, 300, 100]]);
    expect(said.slice(2)).toEqual(["swipe -1"]);
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
