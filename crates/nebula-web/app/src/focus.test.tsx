import { isValidElement, type ReactElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { Board, Session, State } from "./api/contract";
import { CardActs, Home, type HomeActs } from "./Idle";
import { cardAnswers, focusPicks, NOT_ACTING, viewItems } from "./model";
import { Rows } from "./Side";
import { NO_SCHEDULES } from "./schedules";

const session = (id: string, over: Partial<Session>): Session => ({
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

/// A board with nothing that needs you: a running session and a captain at rest.
const quiet = (board?: Board): State => ({
  projects: [],
  sessions: [session("run", { status: "running" }), session("cap", { role: "captain" })],
  board: board ?? {
    sections: [
      { section: "running", rows: [{ sessionId: "run", kind: "session", workers: 0 }] },
      { section: "captains", rows: [{ sessionId: "cap", kind: "captain", workers: 1 }] },
    ],
    lanes: [],
    idle: 0,
    stopped: 0,
  },
  focus: { source: "rule", at: null, picks: [], fallback: null, rank: null },
  orchestratorId: null,
  tui: null,
  refresh: { running: false, at: null, problems: [] },
  today: null,
  todayProblem: null,
  schedules: NO_SCHEDULES,
  server: { version: "test", protocol: 1 },
});

const acts: HomeActs = {
  onOpen: () => {},
  onOpenIssue: () => {},
  onStart: () => {},
  onAnswer: () => {},
  onOption: () => {},
  onSay: async () => null,
  onRank: () => {},
  onGo: () => {},
};

const idle = (state: State) =>
  renderToStaticMarkup(
    <Home
      state={state}
      now={0}
      note={null}
      cursor={null}
      acting={NOT_ACTING}
      rankSaid={null}
      acts={acts}
    />,
  );

const side = (state: State) =>
  renderToStaticMarkup(
    <Rows
      state={state}
      view="inbox"
      items={viewItems(state, "inbox", null)}
      cursor={null}
      openPath={null}
      now={0}
      query=""
      onOpenRow={() => {}}
    />,
  );

const times = (html: string, text: string) => html.split(text).length - 1;

describe("with nothing in focus", () => {
  it("Home says nothing needs you where the cards go, under what the board holds", () => {
    const html = idle(quiet());
    expect(html).not.toContain('class="cards"');
    expect(html).toContain('<p class="calm">Nothing needs you.</p>');
    expect(html).toContain('<p class="day-line">1 running</p>');
  });

  it("Home says it once on an empty board", () => {
    const html = idle(quiet({ sections: [], lanes: [], idle: 2, stopped: 0 }));
    expect(html).toContain('<p class="day-line">No session is running.</p>');
    expect(times(html, "Nothing needs you.")).toBe(1);
  });

  it("the list keeps the running session and the captain in their sections, and leaves the saying to Home", () => {
    const html = side(quiet());
    expect(html).not.toContain("Nothing needs you.");
    expect(html).toMatch(/Running<span class="group-n">1<\/span><\/h2><a class="srow" href="\/s\/run">/);
    expect(html).toMatch(/Captains<span class="group-n">1<\/span><\/h2><a class="srow" href="\/s\/cap">/);
  });

  it("a captain's row counts its workers at work, as the TUI's does, and none while none is", () => {
    expect(side(quiet())).toContain('<span class="srow-meta" title="p · captain · 1 worker">');
    const none = quiet({
      sections: [{ section: "captains", rows: [{ sessionId: "cap", kind: "captain", workers: 0 }] }],
      lanes: [],
      idle: 0,
      stopped: 0,
    });
    expect(side(none)).toContain('<span class="srow-meta" title="p · captain">');
  });
});

/// A board where two sessions need you and one is done, an issue in flight,
/// and the orchestrator's picks: `pick` of them, by key.
const picked = (keys: string[], over: Partial<State["focus"]> = {}): State => ({
  ...quiet(),
  sessions: [
    session("ask", { status: "needsFeedback", why: "Bash: cargo test", statusChangedAt: 0 }),
    session("blocked", { reportWord: "BLOCKED", why: "BLOCKED: the key expired", statusChangedAt: 0 }),
    session("done", { unseen: true, why: "shipped", statusChangedAt: 0 }),
  ],
  board: {
    sections: [
      {
        section: "needsYou",
        rows: [
          { sessionId: "ask", kind: "session", workers: 0 },
          { sessionId: "blocked", kind: "session", workers: 0 },
        ],
      },
      { section: "done", rows: [{ sessionId: "done", kind: "session", workers: 0 }] },
    ],
    lanes: [
      {
        lane: "inFlight",
        issues: [
          {
            key: "ACME-7",
            url: "https://linear.app/acme/issue/ACME-7",
            title: "Retry the upload",
            priority: 1,
            state: "In Progress",
            stateType: "started",
            assignee: null,
            branchName: "acme-7",
            updatedAt: 0,
            team: "ACME",
            project: null,
            started: null,
            lastProject: null,
          },
        ],
      },
    ],
    idle: 0,
    stopped: 0,
  },
  focus: {
    source: "orchestrator",
    at: 0,
    picks: keys.map((key) => ({
      key,
      sessionId: key === "ACME-7" ? null : key,
      reason: `${key} matters`,
      answers: [],
    })),
    fallback: null,
    rank: null,
    ...over,
  },
  orchestratorId: "orch",
});

describe("with the orchestrator's picks", () => {
  it("an issue it picks is a card that opens the issue, with Start and Not now, and it stays in its lane", () => {
    const state = picked(["ACME-7"]);
    expect(focusPicks(state)[0]).toMatchObject({ kind: "focus", key: "ACME-7", sessionId: null });
    expect(viewItems(state, "issues", null).map((i) => i.kind === "issue" && i.issue.key)).toEqual(["ACME-7"]);
    const html = idle(state);
    expect(html).toContain('<a class="card-title" href="/i/ACME-7">Retry the upload</a>');
    expect(html).toContain('<p class="card-why">ACME-7 matters</p>');
    expect(html).toMatch(/<div class="card-acts" role="group" aria-label="Act on ACME-7"><button type="button" class="answer first">[^]*Start<\/button><button type="button" class="answer">Not now<\/button><\/div>/);
  });

  it("keeps the rows that need you and that it left out in sight, and never says nothing needs you", () => {
    const html = idle(picked(["done"]));
    expect(html).toContain("Also needs you");
    expect(html).toContain("not among the orchestrator&#x27;s picks");
    expect(times(html, 'href="/s/ask"')).toBe(1);
    expect(times(html, 'href="/s/blocked"')).toBe(1);
    expect(html).not.toContain("Nothing needs you.");
    expect(html).toContain("The orchestrator picked one at");
  });

  it("picking nothing leaves what needs you under its own head, and the list keeps it under Needs you", () => {
    const state = picked([]);
    const html = idle(state);
    expect(html).not.toContain('class="cards"');
    expect(html).not.toContain("Nothing needs you.");
    expect(html).toContain("the orchestrator picked none of these");
    expect(html).toContain("The orchestrator picked nothing at");
    expect(side(state)).toContain('Needs you<span class="group-n">2</span>');
  });

  it("each pick can be put aside, Not today or Wrong, and none by rule", () => {
    const aside = 'aria-label="Not today, or a wrong pick"';
    expect(times(idle(picked(["ask", "ACME-7"])), aside)).toBe(2);
    expect(times(idle(picked(["ask"], { source: "rule" })), aside)).toBe(0);
  });

  it("Rank again waits on a rank under way, and the Spotlight says where it stands", () => {
    const held = idle(picked(["ask"], { rank: { at: 0, auto: false, state: "held" } }));
    expect(held).toMatch(/<button type="button" class="btn" disabled="" aria-busy="true"[^>]*>[^]*Ranking…<\/button>/);
    expect(held).toContain("Ranking once the orchestrator is at rest with an empty input box.");
    const ranking = idle(picked(["ask"], { rank: { at: 0, auto: true, state: "ranking" } }));
    expect(ranking).toContain("The orchestrator is ranking.");
    const ended = idle(picked(["ask"], { rank: { at: 0, auto: false, state: "ended" } }));
    expect(ended).toMatch(/<button type="button" class="btn" title="[^"]*">[^]*Rank again<\/button>/);
    expect(ended).toContain('<p class="spot-note late">The orchestrator&#x27;s last rank ended');
  });

  it("the rule stands in for stale, gone or set aside picks, and says why", () => {
    const by = (fallback: State["focus"]["fallback"]) => idle({ ...picked(["ask"], { source: "rule", fallback, at: 0 }) });
    expect(by("stale")).toMatch(/picks from [^<]* are over three hours old, and the board has changed since\./);
    expect(by("gone")).toMatch(/None of the orchestrator&#x27;s picks from [^<]* is on the board now\./);
    expect(by("setAside")).toMatch(/You put the orchestrator&#x27;s picks from [^<]* aside\./);
    expect(by("unread")).toContain("show once Linear and the pull requests are read.");
    expect(idle(picked(["ask"], { source: "rule", at: null }))).toContain("The orchestrator has not ranked yet.");
    expect(idle({ ...picked(["ask"], { source: "rule", at: null }), orchestratorId: null })).toMatch(
      /<button type="button" class="btn" disabled=""[^>]*>[^]*Rank again<\/button>/,
    );
  });
});

/// The buttons in `node`, a tree of elements no hook draws.
function buttons(node: ReactNode): ReactElement<{ onClick?: () => void; children?: ReactNode; disabled?: boolean }>[] {
  if (Array.isArray(node)) return node.flatMap(buttons);
  if (!isValidElement<{ children?: ReactNode }>(node)) return [];
  const own = node.type === "button" ? [node as ReactElement<{ onClick?: () => void }>] : [];
  return [...own, ...buttons(node.props.children)];
}

const text = (node: ReactNode): string =>
  Array.isArray(node)
    ? node.map(text).join("")
    : isValidElement<{ children?: ReactNode }>(node)
      ? text(node.props.children)
      : typeof node === "string" || typeof node === "number"
        ? String(node)
        : "";

describe("answering from the Spotlight", () => {
  const answered = () => {
    const state = picked(["blocked", "ACME-7"]);
    state.focus.picks[0]!.answers = ["I added it, run again", "skip the staging run"];
    return state;
  };

  it("draws a card's answers, numbered, the first as the one to press", () => {
    const html = idle(answered());
    expect(html).toContain('<div class="card-acts" role="group" aria-label="Answers for blocked">');
    expect(html).toContain(
      '<button type="button" class="answer first"><span class="answer-n" aria-hidden="true">1</span>I added it, run again</button>',
    );
    expect(html).toContain('<span class="answer-n" aria-hidden="true">2</span>skip the staging run</button>');
  });

  it("sends the answer pressed, as the card's session's follow-up", () => {
    const state = answered();
    const item = focusPicks(state)[0]!;
    const s = state.sessions.find((x) => x.id === "blocked")!;
    const onAnswer = vi.fn();
    const pills = buttons(CardActs({ item, s, busy: false, onAnswer, onStart: () => {}, onNotNow: null }));
    expect(pills.map((b) => text(b.props.children))).toEqual(["1I added it, run again", "2skip the staging run"]);
    pills[1]!.props.onClick!();
    expect(onAnswer).toHaveBeenCalledWith(item, 2);
    expect(cardAnswers(item, s)[2 - 1]).toBe("skip the staging run");
  });

  it("offers no answer while the card's session runs, and Start on an issue no session works on", () => {
    const state = answered();
    const item = focusPicks(state)[0]!;
    const s = { ...state.sessions.find((x) => x.id === "blocked")!, status: "running" as const };
    expect(CardActs({ item, s, busy: false, onAnswer: () => {}, onStart: () => {}, onNotNow: null })).toBeNull();
    const onStart = vi.fn();
    const issue = focusPicks(state)[1]!;
    const [start] = buttons(CardActs({ item: issue, s: undefined, busy: false, onAnswer: () => {}, onStart, onNotNow: null }));
    start!.props.onClick!();
    expect(onStart).toHaveBeenCalledWith("ACME-7");
  });
});
