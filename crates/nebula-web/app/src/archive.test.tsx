import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Session, State } from "./api/contract";
import { ArchivedSpot, Detail, type Layout } from "./Detail";
import { archiveAsk, archivedText, type Archived } from "./model";
import { NO_SCHEDULES } from "./schedules";

const NOW = 100 * 86_400_000;

const session = (id: string, over: Partial<Session> = {}): Session => ({
  id,
  name: id,
  projectId: "p",
  projectName: "rover-firmware",
  branch: "main",
  rootCheckout: false,
  harness: "claude",
  role: "worker",
  status: "finished",
  statusChangedAt: NOW - 60_000,
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

const state = (sessions: Session[]): State => ({
  projects: [],
  sessions,
  board: { sections: [], lanes: [], idle: 0, stopped: 0 },
  focus: { source: "rule", at: null, picks: [], fallback: null, rank: null },
  orchestratorId: null,
  tui: null,
  refresh: { running: false, at: NOW, problems: [] },
  today: null,
  todayProblem: null,
  schedules: NO_SCHEDULES,
  server: { version: "test", protocol: 66 },
});

function page(s: Session, sessions: Session[] = [s], layout: Layout = "wide") {
  return renderToStaticMarkup(
    <Detail
      state={state(sessions)}
      session={s}
      detail={null}
      turns={null}
      pick={null}
      note={null}
      onSay={null}
      focus={state([]).focus}
      now={NOW}
      busy={false}
      error={null}
      layout={layout}
      back="Inbox"
      terminal={false}
      canShow={false}
      shown={null}
      follow={null}
      banner={null}
      onBack={() => {}}
      onTerminal={() => {}}
      onShow={() => {}}
      onFollow={() => {}}
      onSeen={() => {}}
      onPick={() => {}}
      onSend={() => Promise.resolve(true)}
      onOpen={() => {}}
      onArchive={() => {}}
    />,
  );
}

const KEEPS =
  "The agent stops and the session leaves the list. Its worktree and branch stay: Undo brings it back, as does Unarchive in the TUI.";

describe("Archive", () => {
  it("says what it stops, what it keeps and how it comes back", () => {
    expect(archiveAsk(session("w", { name: "GPS link recovery" }), state([]))).toEqual({
      title: "Archive GPS link recovery?",
      says: [KEEPS],
    });
  });

  it("says a captain's workers keep running and only the captain goes", () => {
    const cap = session("cap", { role: "captain", name: "Rover captain" });
    const other = session("elsewhere", { projectId: "q" });
    expect(archiveAsk(cap, state([cap, other])).says).toEqual([KEEPS]);
    expect(archiveAsk(cap, state([cap, session("w1"), other])).says).toEqual([
      KEEPS,
      "Its worker keeps running: only the captain is archived.",
    ]);
    expect(archiveAsk(cap, state([cap, session("w1"), session("w2"), other])).says[1]).toBe(
      "Its 2 workers keep running: only the captain is archived.",
    );
  });

  it("is in the ⋯ menu of a worker and a captain, wide and on a phone, with its confirm", () => {
    const cap = session("cap", { role: "captain", name: "Rover captain" });
    for (const layout of ["wide", "phone"] as const) {
      for (const s of [session("w", { name: "GPS link recovery" }), cap]) {
        const html = page(s, [s, session("w2")], layout);
        expect(html).toMatch(/<button type="button" class="menu-item"><svg[^>]*data-icon="archive"[^]*?<\/svg>Archive<\/button>/);
        expect(html).toContain(`<h2 id="archive`);
        expect(html).toContain(`>Archive ${s.name}?</h2>`);
        expect(html).toContain(`<p>${KEEPS}</p>`);
        expect(html).toContain(
          '<div class="archive-acts"><button type="button" class="btn">Cancel</button><button type="button" class="btn primary archive-yes">Archive</button></div>',
        );
        expect(html).toContain(layout === "phone" ? 'class="phones archive-ask sheet"' : 'class="phones archive-ask"');
      }
    }
    expect(page(cap, [cap, session("w2")])).toContain("<p>Its worker keeps running: only the captain is archived.</p>");
  });

  it("is not offered for the orchestrator, which F10 in the TUI starts and stops", () => {
    const orch = session("orch", { name: "orchestrator", role: "orchestrator" });
    const html = page(orch, [orch], "phone");
    expect(html).not.toContain("Archive");
    expect(html).not.toContain("<dialog");
  });

  it("says at the page's foot what it did, with Undo while it can be undone", () => {
    const a: Archived = { id: "w", name: "GPS link recovery", seq: 1, phase: "archiving", error: null };
    expect(archivedText(a)).toEqual({ text: "Archiving GPS link recovery…", undo: false });
    expect(archivedText({ ...a, phase: "archived" })).toEqual({ text: "Archived GPS link recovery.", undo: true });
    expect(archivedText({ ...a, phase: "undoing" })).toEqual({ text: "Archived GPS link recovery.", undo: true });
    expect(archivedText({ ...a, phase: "back" })).toEqual({ text: "Brought GPS link recovery back.", undo: false });
    expect(archivedText({ ...a, phase: "failed", error: "No nebula daemon is running." })).toEqual({
      text: "No nebula daemon is running.",
      undo: false,
    });

    const spot = (archived: Archived | null) => renderToStaticMarkup(<ArchivedSpot archived={archived} onUndo={() => {}} />);
    expect(spot(null)).toBe('<div class="toast-spot" role="status"></div>');
    expect(spot(a)).toBe('<div class="toast-spot" role="status"><p class="toast"><span class="toast-text">Archiving GPS link recovery…</span></p></div>');
    expect(spot({ ...a, phase: "archived" })).toContain('<button type="button" class="toast-undo">Undo</button>');
    expect(spot({ ...a, phase: "undoing" })).toContain('<button type="button" class="toast-undo" disabled="" aria-busy="true">Undo</button>');
    expect(spot({ ...a, phase: "failed", error: "No." })).toContain('<p class="toast error">');
  });
});
