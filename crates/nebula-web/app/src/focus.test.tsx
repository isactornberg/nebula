import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Board, Session, State } from "./api/contract";
import { Idle } from "./Idle";
import { listItems } from "./model";
import { Side } from "./Side";

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
  server: { version: "test", protocol: 1 },
});

const idle = (state: State) =>
  renderToStaticMarkup(
    <Idle
      state={state}
      items={listItems(state)}
      cursor={null}
      now={0}
      note={null}
      busy={false}
      error={null}
      actedOn={null}
      onOpen={() => {}}
      onOpenIssue={() => {}}
      onAnswer={() => {}}
      onSay={async () => null}
    />,
  );

const side = (state: State) =>
  renderToStaticMarkup(
    <Side
      state={state}
      items={listItems(state)}
      cursor={null}
      openPath={null}
      now={0}
      refreshSaid={null}
      rankSaid={null}
      onOpenRow={() => {}}
      onRefresh={() => {}}
      onRank={() => {}}
    />,
  );

const times = (html: string, text: string) => html.split(text).length - 1;

describe("with nothing in focus", () => {
  it("the idle page says nothing needs you where the cards go, under a summary of the board", () => {
    const html = idle(quiet());
    expect(html).not.toContain('class="cards"');
    expect(html).toContain('<p class="calm">Nothing needs you.</p>');
    expect(html).toContain('<p class="muted">One session is running.</p>');
  });

  it("the idle page says it once on an empty board", () => {
    const html = idle(quiet({ sections: [], lanes: [], idle: 2, stopped: 0 }));
    expect(html).toContain('<p class="muted">No session is running.</p>');
    expect(times(html, "Nothing needs you.")).toBe(1);
  });

  it("the list says it under Focus and keeps the running session and the captain in their lanes", () => {
    const html = side(quiet());
    expect(html).toMatch(/id="focus-title"[^]*<p class="side-note">Nothing needs you.<\/p><\/section>/);
    expect(times(html, "Nothing needs you.")).toBe(1);
    expect(html).toMatch(/>Running 1<\/h2><a class="row lane-row" href="\/s\/run">/);
    expect(html).toMatch(/>Captains 1<\/h2><a class="row lane-row" href="\/s\/cap">/);
  });

  it("a captain's row counts its workers at work, as the TUI's does, and none while none is", () => {
    expect(side(quiet())).toContain('<span class="title" title="cap · 1 worker">cap · 1 worker</span>');
    const none = quiet({
      sections: [{ section: "captains", rows: [{ sessionId: "cap", kind: "captain", workers: 0 }] }],
      lanes: [],
      idle: 0,
      stopped: 0,
    });
    expect(side(none)).toContain('<span class="title" title="cap">cap</span>');
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
  it("an issue it picks is a card that opens the issue, and leaves its lane", () => {
    const state = picked(["ACME-7"]);
    const items = listItems(state);
    expect(items[0]).toMatchObject({ kind: "focus", key: "ACME-7", sessionId: null });
    expect(items.some((i) => i.kind === "issue")).toBe(false);
    const html = idle(state);
    expect(html).toContain('<h2>Retry the upload</h2><p class="reason">ACME-7 matters</p>');
    expect(html).toContain('aria-label="Open ACME-7" href="/i/ACME-7"');
    expect(side(state)).toContain('<a class="row focus-row" href="/i/ACME-7">');
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

  it("picking nothing leaves what needs you under its own head, and the list says it picked nothing", () => {
    const state = picked([]);
    const html = idle(state);
    expect(html).not.toContain('class="cards"');
    expect(html).not.toContain("Nothing needs you.");
    expect(html).toContain("the orchestrator picked none of these");
    expect(html).toContain("The orchestrator picked nothing at");
    const list = side(state);
    expect(list).toContain('<p class="side-note">The orchestrator picked nothing.</p>');
    expect(list).toMatch(/>Needs you 2<\/h2>/);
  });

  it("each pick takes Not today and Wrong, and none by rule", () => {
    expect(times(idle(picked(["ask", "ACME-7"])), ">Not today</button>")).toBe(2);
    expect(times(idle(picked(["ask"], { source: "rule" })), ">Not today</button>")).toBe(0);
  });

  it("Re-rank waits on a rank under way, and the list says where it stands", () => {
    const held = side(picked(["ask"], { rank: { at: 0, auto: false, state: "held" } }));
    expect(held).toMatch(/<button type="button" class="btn" disabled="" aria-busy="true"[^>]*>Ranking…<\/button>/);
    expect(held).toContain("Ranking once the orchestrator is at rest with an empty input box.");
    const ranking = side(picked(["ask"], { rank: { at: 0, auto: true, state: "ranking" } }));
    expect(ranking).toContain("The orchestrator is ranking.");
    const ended = side(picked(["ask"], { rank: { at: 0, auto: false, state: "ended" } }));
    expect(ended).toMatch(/<button type="button" class="btn" title="[^"]*">Re-rank<\/button>/);
    expect(ended).toContain('<p class="side-note focus-note late">The orchestrator&#x27;s last rank ended');
  });

  it("the rule stands in for stale, gone or set aside picks, and says why", () => {
    const by = (fallback: State["focus"]["fallback"]) =>
      side({ ...picked(["ask"], { source: "rule", fallback, at: 0 }) });
    expect(by("stale")).toMatch(/picks from [^<]* are over three hours old, and the board has changed since\./);
    expect(by("gone")).toMatch(/None of the orchestrator&#x27;s picks from [^<]* is on the board now\./);
    expect(by("setAside")).toMatch(/You put the orchestrator&#x27;s picks from [^<]* aside\./);
    expect(by("unread")).toContain("show once Linear and the pull requests are read.");
    expect(side(picked(["ask"], { source: "rule", at: null }))).toContain("The orchestrator has not ranked yet.");
    expect(side({ ...picked(["ask"], { source: "rule", at: null }), orchestratorId: null })).toMatch(
      /<button type="button" class="btn" disabled=""[^>]*>Re-rank<\/button>/,
    );
  });
});
