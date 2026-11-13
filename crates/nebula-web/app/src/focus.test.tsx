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
  focus: { source: "rule", at: null, picks: [] },
  orchestratorId: null,
  tui: null,
  refresh: { running: false, at: null, problems: [] },
  today: null,
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
      onAnswer={() => {}}
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
      onOpenRow={() => {}}
      onRefresh={() => {}}
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
