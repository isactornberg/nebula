import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Issue, Session, State } from "./api/contract";
import { IssueDetail } from "./Detail";
import { viewItems } from "./model";
import { RefreshLine, Rows } from "./Side";
import { NO_SCHEDULES } from "./schedules";

const MIN = 60_000;
/// 11:00 UTC, the same day as two minutes before in every time zone.
const NOW = 100 * 86_400_000 + 11 * 3_600_000;

const issue = (key: string, over: Partial<Issue> = {}): Issue => ({
  key,
  url: `https://linear.app/acme/issue/${key}`,
  title: `${key} title`,
  priority: 2,
  state: "In Progress",
  stateType: "started",
  assignee: "Ada",
  branchName: `ada/${key.toLowerCase()}-work`,
  updatedAt: NOW - 3 * 60 * MIN,
  team: "ACME",
  project: "Rover",
  started: null,
  lastProject: null,
  ...over,
});

const session = (id: string, over: Partial<Session> = {}): Session => ({
  id,
  name: id,
  projectId: "p",
  projectName: "p",
  branch: "acme-7-fix",
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

const state = (over: Partial<State> = {}): State => ({
  projects: [],
  sessions: [session("run", { status: "running" })],
  board: {
    sections: [{ section: "running", rows: [{ sessionId: "run", kind: "session", workers: 0 }] }],
    lanes: [
      { lane: "inFlight", issues: [issue("ACME-1", { priority: 1 })] },
      { lane: "todo", issues: [issue("ACME-2", { stateType: "unstarted", priority: 0 })] },
    ],
    idle: 0,
    stopped: 0,
  },
  focus: { source: "rule", at: null, picks: [], fallback: null, rank: null },
  orchestratorId: null,
  tui: null,
  refresh: { running: false, at: NOW - 4 * MIN, problems: [] },
  today: null,
  todayProblem: null,
  schedules: NO_SCHEDULES,
  server: { version: "test", protocol: 1 },
  ...over,
});

const side = (s: State, openPath: string | null = null) =>
  renderToStaticMarkup(
    <Rows
      state={s}
      view="issues"
      items={viewItems(s, "issues", null)}
      cursor={null}
      openPath={openPath}
      now={NOW}
      query=""
      onOpenRow={() => {}}
    />,
  );

const refresh = (s: State) =>
  renderToStaticMarkup(<RefreshLine state={s} now={NOW} said={null} onRefresh={() => {}} />);

describe("the issue lanes", () => {
  it("are the Issues view, lane by lane, each row its priority, title, key and state, opening the issue", () => {
    const html = side(state(), "/i/ACME-1");
    expect(html).toMatch(/In flight, no session<span class="group-n">1<\/span>[^]*Todo<span class="group-n">1<\/span>/);
    expect(html).toContain(
      '<a class="srow issue-row" aria-current="page" href="/i/ACME-1"><svg class="priority p1" role="img" aria-label="Urgent"',
    );
    expect(html).toContain('<span class="srow-title" title="ACME-1 title">ACME-1 title</span><span class="srow-age">3h</span>');
    expect(html).toContain('<span class="srow-meta"><span class="m">ACME-1</span> · In Progress</span>');
    expect(html).toContain('<svg class="priority p0" role="img" aria-label="No priority"');
  });

  it("say why they are empty: no Linear team, not read yet, nothing waiting, or what the read could not reach", () => {
    const unset = state({ board: { ...state().board, lanes: [] } });
    expect(side(unset)).toContain("No project reads its issues from Linear.");
    const none = { ...unset, projects: [{ id: "p", name: "p", space: null, team: "ACME" }] };
    expect(side(none)).not.toContain("No project reads its issues from Linear.");
    expect(side(none)).toContain("No issue assigned to you waits for a session.");
    expect(side({ ...none, refresh: { running: false, at: null, problems: [] } })).toContain("Linear&#x27;s issues are not read yet.");
    const failed = side({ ...none, refresh: { running: false, at: NOW, problems: ["LINEAR_API_KEY is not set."] } });
    expect(failed).toContain('<li class="error">LINEAR_API_KEY is not set.</li>');
  });
});

describe("Refresh", () => {
  it("says when the issues and pull requests were read, beside a button that shows its key", () => {
    const html = refresh(state());
    expect(html).toContain('<span class="refresh-when">Updated 4 min ago</span><span class="sr" role="status">Refreshed.</span>');
    expect(html).toMatch(
      /<button type="button" class="quiet-btn" aria-keyshortcuts="r" title="Reads Linear&#x27;s issues assigned to you and the sessions&#x27; pull requests again.">[^]*Refresh <kbd aria-hidden="true">r<\/kbd><\/button>/,
    );
    expect(html).not.toContain("refresh-problems");
  });

  it("says it is under way, and why each part of the last one failed", () => {
    const problems = ["LINEAR_API_KEY is not set in nebula serve's environment.", "Pull requests in api: gh timed out."];
    const html = refresh(state({ refresh: { running: true, at: NOW - MIN, problems } }));
    expect(html).toContain('<span class="refresh-when">Refreshing…</span><span class="sr" role="status">Refreshing…</span>');
    expect(html).toContain('aria-busy="true"');
    expect(html).toContain(
      '<ul class="refresh-problems"><li class="error">LINEAR_API_KEY is not set in nebula serve&#x27;s environment.</li><li class="error">Pull requests in api: gh timed out.</li></ul>',
    );
  });
});

describe("an issue's detail", () => {
  const projects = [
    { id: "p1", name: "api", space: "Acme", team: "ACME" },
    { id: "p2", name: "web", space: "Acme", team: "ACME" },
    { id: "p3", name: "docs", space: null, team: "DOC" },
  ];
  const detail = (i: Issue, over: { asking?: boolean; sessions?: Session[]; error?: string | null; phone?: boolean } = {}) =>
    renderToStaticMarkup(
      <IssueDetail
        issue={i}
        state={state({ projects, sessions: over.sessions ?? [] })}
        now={NOW}
        asking={over.asking ?? false}
        busy={false}
        error={over.error ?? null}
        layout={over.phone ? "phone" : "wide"}
        back="Issues"
        onBack={() => {}}
        onAsk={() => {}}
        onStart={() => {}}
        pick={null}
        note={null}
        onSay={null}
        banner={null}
      />,
    );
  const text = (html: string) => html.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");

  it("shows its title, key, state, priority, project and when it changed, with a link to it and Start", () => {
    const html = detail(issue("ACME-1", { priority: 1 }));
    expect(html).toContain('<h1 class="sess-title">ACME-1 title</h1>');
    expect(html).toContain('<a class="btn" href="https://linear.app/acme/issue/ACME-1" target="_blank" rel="noreferrer">Linear ↗</a>');
    expect(html).toContain('<span class="state-pill tint-peach">Urgent</span>');
    expect(text(html)).toContain("State In Progress Priority Urgent Urgent Project Rover Updated");
    expect(html).toContain('<span class="facts-none">· 3 hours ago</span>');
    expect(html).toMatch(/<button [^>]*aria-keyshortcuts="s"[^>]*>[^]*Start<kbd aria-hidden="true">S<\/kbd><\/button>/);
    expect(html).toMatch(/Session<\/span><span class="life-main life-none">No session yet/);
    expect(html).not.toContain("start-ask");
  });

  it("asks first, naming the issue, the project it goes to, what that does there and in Linear, with its own Start and a Cancel", () => {
    const html = detail(issue("ACME-2", { stateType: "unstarted", state: "Todo", lastProject: "p2" }), { asking: true });
    expect(html).toContain('<div class="start-ask" role="group" aria-labelledby="start-ask"><p id="start-ask" class="start-q">Start ACME-2 in web?</p>');
    expect(html).toMatch(/<button type="button" aria-pressed="false">api<\/button><button type="button" aria-pressed="true">web<\/button>/);
    expect(html).not.toContain(">docs<");
    expect(text(html)).toContain(
      "web has no captain yet, so nebula starts one, which starts a worker on the issue, on a branch of its own. In Linear the issue moves from Todo to its team&#x27;s first started state.",
    );
    expect(html).toContain('<button type="button" class="btn primary">Start ACME-2</button><button type="button" class="btn">Cancel</button>');
    expect(html).not.toMatch(/aria-keyshortcuts="s"/);
  });

  it("offers the one project of its team without a choice, and says its captain's brief waits while it works", () => {
    const captain = session("cap", { role: "captain", projectId: "p3", status: "running" });
    const html = detail(issue("DOC-1", { team: "DOC" }), { asking: true, sessions: [captain] });
    expect(html).toContain(">Start DOC-1 in docs?</p>");
    expect(html).not.toContain("start-projects");
    expect(text(html)).toContain(
      "Its captain starts a worker on the issue, on a branch of its own, once it is done with what it is at: the brief waits until then. It is In Progress in Linear already, and stays there.",
    );
  });

  it("says once it is handed to which captain and when, whether the brief waits, and why Linear was not moved", () => {
    const started = { projectId: "p1", projectName: "api", captainId: "cap", at: NOW - 2 * MIN, held: true, moveFailed: "Linear timed out." };
    const html = detail(issue("ACME-3", { started }));
    expect(html).toMatch(/<p class="issue-line handed">Handed to the captain of api at \d\d:\d\d\. The brief waits until the captain is at rest with an empty input box\.<\/p>/);
    expect(html).toContain('<p class="issue-line move-failed">It was not moved to its started state in Linear: Linear timed out.</p>');
    expect(html).toContain("Handed to api, brief held");
    expect(html).not.toContain("no session works on it");
    const again = detail(issue("ACME-3", { started }), { asking: true, error: "The daemon did not answer." });
    expect(again).toContain(">ACME-3 was handed to the captain of api already. Start it again in api?</p>");
    expect(again).toContain(">Start ACME-3 again</button>");
    expect(again).toContain('<p class="error" role="alert">The daemon did not answer.</p>');
  });

  it("has no Start when no project reads its team", () => {
    const html = detail(issue("OPS-1", { team: "OPS" }));
    expect(html).not.toMatch(/>Start/);
    expect(html).toContain("No project reads its issues from Linear&#x27;s OPS team, so Start has nowhere to hand it.");
  });

  it("on a phone, goes back to the list and has Start at the foot, a thumb's reach", () => {
    const html = detail(issue("ACME-1"), { phone: true });
    expect(html).toMatch(/<a class="sess-back" href="\/">[^]*Issues<\/a>/);
    expect(html).toMatch(/<div class="dock"><div class="dock-in"><button type="button" class="dock-start" aria-keyshortcuts="s"[^>]*>[^]*Start ACME-1<\/button>/);
  });

  it("is said on its row once handed over: to which captain, and how long ago or that its brief waits", () => {
    const started = { projectId: "p1", projectName: "api", captainId: "cap", at: NOW - 2 * MIN, held: false, moveFailed: null };
    const lanes = (held: boolean) =>
      side(state({ board: { ...state().board, lanes: [{ lane: "todo", issues: [issue("ACME-4", { started: { ...started, held } })] }] } }));
    expect(lanes(false)).toMatch(
      /<span class="handed" title="Handed to the captain of api at \d\d:\d\d\. The page goes to its worker once the captain starts it\."><span class="handed-to">handed to api<\/span><span aria-hidden="true">·<\/span><span>2m<\/span><\/span>/,
    );
    expect(lanes(true)).toContain('<span class="handed-to">handed to api</span><span aria-hidden="true">·</span><span class="held">brief held</span>');
  });
});
