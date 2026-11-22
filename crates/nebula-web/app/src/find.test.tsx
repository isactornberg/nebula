import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Issue, Session, State } from "./api/contract";
import { findItems, itemRoute, keyAction, ORCHESTRATOR_LINE, viewItems, type ListItem } from "./model";
import { FindField, Rows } from "./Side";
import { NO_SCHEDULES } from "./schedules";

const HOUR = 3_600_000;
const NOW = 100 * 86_400_000;

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
  statusChangedAt: NOW - 2 * HOUR,
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

const issue = (key: string): Issue => ({
  key,
  url: `https://linear.app/acme/issue/${key}`,
  title: `${key} title`,
  priority: 2,
  state: "In Progress",
  stateType: "started",
  assignee: "Ada",
  branchName: key.toLowerCase(),
  updatedAt: 1,
  team: "ACME",
  project: null,
  started: null,
  lastProject: null,
});

/// Three sessions on the board, and four it leaves off: two idle, one
/// in a captain's project, one stopped, and the orchestrator.
const state = (): State => ({
  projects: [],
  sessions: [
    session("asks", { name: "Fix the login form", projectName: "shopfront", branch: "acme-123-signin", status: "needsFeedback" }),
    session("run", { name: "Rotate the staging key", projectName: "api", branch: "rotate", status: "running" }),
    session("idle", {
      name: "Tidy the README",
      projectName: "docs",
      rootCheckout: true,
      issueUrl: "https://github.com/acme/docs/issues/212",
      issueKey: "#212",
    }),
    session("stopped", { name: "Port the billing page", projectName: "shopfront", branch: "billing", status: "terminated" }),
    session("rested", { name: "Cache the results", projectName: "api", branch: "perf", issue: issue("ACME-77") }),
    session("cap", { name: "captain", projectName: "api", branch: "captain", role: "captain" }),
    session("orch", { name: "orchestrator", projectName: "api", role: "orchestrator" }),
  ],
  board: {
    sections: [
      { section: "needsYou", rows: [{ sessionId: "asks", kind: "session", workers: 0 }] },
      { section: "running", rows: [{ sessionId: "run", kind: "session", workers: 0 }] },
      { section: "captains", rows: [{ sessionId: "cap", kind: "captain", workers: 0 }] },
    ],
    lanes: [],
    idle: 2,
    stopped: 1,
  },
  focus: { source: "rule", at: null, picks: [{ key: "asks", sessionId: "asks", reason: "asks", answers: [] }], fallback: null, rank: null },
  orchestratorId: "orch",
  tui: null,
  refresh: { running: false, at: NOW, problems: [] },
  today: null,
  todayProblem: null,
  schedules: NO_SCHEDULES,
  server: { version: "test", protocol: 1 },
});

const found = (query: string, over: Partial<State> = {}) =>
  findItems({ ...state(), ...over }, query).map((i: ListItem) => (i.kind === "row" ? i.sessionId : i.issue.key));

describe("the find", () => {
  it("matches a session's name, project, branch and issue key", () => {
    expect(found("login")).toEqual(["asks"]);
    expect(found("docs")).toEqual(["idle"]);
    expect(found("signin")).toEqual(["asks"]);
    expect(found("#212")).toEqual(["idle"]);
    expect(found("acme-77")).toEqual(["rested"]);
  });

  it("is blind to case", () => {
    expect(found("LOGIN")).toEqual(["asks"]);
    expect(found("ShopFront")).toEqual(["asks", "stopped"]);
  });

  it("wants every word, in any order and in any of the fields", () => {
    expect(found("shopfront billing")).toEqual(["stopped"]);
    expect(found("billing  shopfront ")).toEqual(["stopped"]);
    expect(found("api acme-77 cache")).toEqual(["rested"]);
    expect(found("login billing")).toEqual([]);
  });

  it("reaches the sessions the board leaves off, after its rows, and the orchestrator last, at its thread's path", () => {
    expect(found("api")).toEqual(["run", "cap", "rested", "orch"]);
    expect(found("the")).toEqual(["asks", "run", "idle", "stopped", "rested"]);
    expect(found("orchestrator")).toEqual(["orch"]);
    expect(itemRoute(findItems(state(), "orchestrator")[0]!)).toEqual({ kind: "orchestrator" });
    expect(itemRoute(findItems(state(), "captain")[0]!)).toEqual({ kind: "session", id: "cap" });
    expect(found("orchestrator", { orchestratorId: null })).toEqual([]);
  });

  it("matches nothing for words no session has, and for none at all", () => {
    expect(found("nowhere")).toEqual([]);
    expect(found("")).toEqual([]);
    expect(found("   ")).toEqual([]);
  });

  it("is what `/` goes to", () => {
    expect(keyAction({ key: "/", metaKey: false, ctrlKey: false, altKey: false })).toEqual({ kind: "find" });
  });
});

/// The list's find field and what it lists, as the page draws them.
const side = (query: string) => {
  const s = state();
  return renderToStaticMarkup(
    <>
      <FindField query={query} find={{ current: null }} onQuery={() => {}} onKey={() => {}} />
      <Rows
        state={s}
        view="inbox"
        items={query.trim() ? findItems(s, query) : viewItems(s, "inbox", null)}
        cursor={null}
        openPath={null}
        now={NOW}
        query={query}
        onOpenRow={() => {}}
      />
    </>,
  );
};

describe("the list while a find is typed", () => {
  it("is the board, and the field offers `/`, while nothing is typed", () => {
    const html = side("");
    expect(html).toContain('<nav class="rows" aria-label="Inbox">');
    expect(html).toContain('Needs you<span class="group-n">1</span>');
    expect(html).toContain("2 idle · 1 stopped");
    expect(html).toContain('<kbd aria-hidden="true">/</kbd>');
    expect(html).not.toContain("Clear the find");
  });

  it("is the matches alone, each with its status, branch and age, and ✕ clears it", () => {
    const html = side("shopfront");
    expect(html).toContain('<nav class="rows" aria-label="Matches">');
    expect(html).toContain('Matches<span class="group-n">2</span>');
    expect(html).toContain('<span class="tag tint-peach">Dialog</span>');
    expect(html).toContain("Fix the login form");
    expect(html).toContain('<span class="tag tint-plain">Stopped</span>');
    expect(html).toContain("Port the billing page");
    expect(html).toContain('title="shopfront · billing"');
    expect(html).toContain(">2h<");
    expect(html).toContain('href="/s/stopped"');
    expect(html).toContain('aria-label="Clear the find"');
    expect(html).not.toContain("Needs you<");
    expect(html).not.toContain("2 idle");
  });

  it("names the issue beside a branch that does not hold its key, and no branch that is the name", () => {
    expect(side("#212")).toContain('title="docs · #212 · main"');
    expect(side("signin")).not.toContain("ACME-123 ·");
    expect(side("captain")).toContain('<span class="srow-meta" title="api">api</span>');
  });

  it("opens the orchestrator's match at its thread, and says what it is", () => {
    const html = side("orchestrator");
    expect(html).toContain('href="/orchestrator"');
    expect(html).toContain(`title="${ORCHESTRATOR_LINE}"`);
  });

  it("says plainly when nothing matches", () => {
    const html = side("nowhere");
    expect(html).toContain("No session has “nowhere” in its name, project, branch or issue.");
    expect(html).not.toContain('class="group-head"');
    expect(side(" login   nowhere")).toContain("No session has every word of “login nowhere” in its name, project, branch or issue.");
  });
});
