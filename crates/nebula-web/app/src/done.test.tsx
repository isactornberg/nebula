import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { BoardSection, SectionName, Session, State } from "./api/contract";
import { doneIndex, keepDone, viewItems, type Held, type ListItem } from "./model";
import { Rows } from "./Side";
import { NO_SCHEDULES } from "./schedules";

const NOW = 100 * 86_400_000;
const FINISHED_AT = NOW - 60_000;

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
  statusChangedAt: FINISHED_AT,
  unseen: true,
  why: null,
  reportWord: null,
  issueUrl: null,
  issueKey: null,
  issue: null,
  prUrl: null,
  prState: null,
  ...over,
});

/// A state whose board has `sections`, each `[name, ids]`, an id led by `cap` a captain's row.
const state = (sessions: Session[], sections: [SectionName, string[]][]): State => ({
  projects: [],
  sessions,
  board: {
    sections: sections.map(
      ([section, ids]): BoardSection => ({
        section,
        rows: ids.map((sessionId) => ({ sessionId, kind: sessionId.startsWith("cap") ? "captain" : "session", workers: 0 })),
      }),
    ),
    lanes: [],
    idle: 0,
    stopped: 0,
  },
  focus: { source: "rule", at: null, picks: [], fallback: null, rank: null },
  orchestratorId: null,
  tui: null,
  refresh: { running: false, at: NOW, problems: [] },
  day: null,
  dayProblem: null,
  schedules: NO_SCHEDULES,
  server: { version: "test", protocol: 66 },
});

/// An item as `done:b`, with a `~` when it is leaving.
const said = (i: ListItem) => (i.kind === "row" ? `${i.group}:${i.sessionId}${i.leaving ? "~" : ""}` : i.issue.key);
const inbox = (s: State) => viewItems(s, "inbox", null);
const held = (id: string, at: number): Held => ({ id, at, changed: FINISHED_AT });

describe("a Done row once its session is open", () => {
  const sessions = ["a", "b", "c", "d"].map((id) => session(id));
  const unread = state(sessions, [
    ["needsYou", ["a"]],
    ["done", ["b", "c"]],
    ["running", ["d"]],
  ]);
  // Opening b read it: the board has it in no section now.
  sessions[1] = { ...sessions[1]!, unseen: false };
  const read = state(sessions, [
    ["needsYou", ["a"]],
    ["done", ["c"]],
    ["running", ["d"]],
  ]);

  it("is where the board had it while unread", () => {
    expect(doneIndex(inbox(unread), "c")).toBe(1);
    expect(doneIndex(inbox(unread), "a")).toBe(-1);
    expect(keepDone(inbox(unread), unread, held("b", 0), null)).toEqual(inbox(unread));
  });

  it("stays in its place in Done while open, after reading it took it out", () => {
    expect(inbox(read).map(said)).toEqual(["needsYou:a", "done:c", "running:d"]);
    expect(keepDone(inbox(read), read, held("b", 0), null).map(said)).toEqual(["needsYou:a", "done:b", "done:c", "running:d"]);
    expect(keepDone(inbox(read), read, held("b", 5), null).map(said)).toEqual(["needsYou:a", "done:c", "done:b", "running:d"]);
  });

  it("folds away from that place once another opens or the list does", () => {
    expect(keepDone(inbox(read), read, null, held("b", 0)).map(said)).toEqual(["needsYou:a", "done:b~", "done:c", "running:d"]);
    expect(keepDone(inbox(read), read, null, null).map(said)).toEqual(["needsYou:a", "done:c", "running:d"]);
  });

  it("brings Done back between Needs you and Running when it was Done's only row", () => {
    const s = state([session("a"), session("b", { unseen: false }), session("d")], [
      ["needsYou", ["a"]],
      ["running", ["d"]],
    ]);
    expect(keepDone(inbox(s), s, held("b", 0), null).map(said)).toEqual(["needsYou:a", "done:b", "running:d"]);
    const alone = state([session("b", { unseen: false })], []);
    expect(keepDone(inbox(alone), alone, held("b", 0), null).map(said)).toEqual(["done:b"]);
  });

  it("keeps a captain in Done while open, and under Captains as its Done row folds", () => {
    const cap = session("cap", { role: "captain", unseen: false });
    const s = state([session("a"), cap], [
      ["needsYou", ["a"]],
      ["captains", ["cap"]],
    ]);
    const open = keepDone(inbox(s), s, held("cap", 0), null);
    expect(open.map(said)).toEqual(["needsYou:a", "done:cap"]);
    expect(open[1]).toMatchObject({ captain: true });
    expect(keepDone(inbox(s), s, null, held("cap", 0)).map(said)).toEqual(["needsYou:a", "done:cap~", "captains:cap"]);
  });

  it("goes where its session's state puts it once another turn starts, or it asks", () => {
    const running = state([session("b", { status: "running", statusChangedAt: NOW, unseen: false })], [["running", ["b"]]]);
    expect(keepDone(inbox(running), running, held("b", 0), null).map(said)).toEqual(["running:b"]);
    const asks = state([session("b", { reportWord: "QUESTION", unseen: false })], [["needsYou", ["b"]]]);
    expect(keepDone(inbox(asks), asks, held("b", 0), null).map(said)).toEqual(["needsYou:b"]);
    const again = state([session("b", { statusChangedAt: NOW, unseen: false })], []);
    expect(keepDone(inbox(again), again, held("b", 0), null)).toEqual([]);
  });

  const rows = (s: State, items: ListItem[], openPath: string | null) =>
    renderToStaticMarkup(
      <Rows state={s} view="inbox" items={items} cursor={null} openPath={openPath} now={NOW} query="" onOpenRow={() => {}} />,
    );

  it("is drawn open, and folding, out of reach and out of Done's count", () => {
    const open = rows(read, keepDone(inbox(read), read, held("b", 0), null), "/s/b");
    expect(open).toContain('Done<span class="group-n">2</span>');
    expect(open).toContain('<a class="srow" aria-current="page" href="/s/b">');
    expect(open).not.toContain("data-leaving");
    const folding = rows(read, keepDone(inbox(read), read, null, held("b", 0)), "/s/c");
    expect(folding).toContain('Done<span class="group-n">1</span>');
    expect(folding).toContain('<a class="srow" data-leaving="true" inert="" href="/s/b">');
  });

  it("folds its whole group when it was Done's only row", () => {
    const s = state([session("a"), session("b", { unseen: false })], [["needsYou", ["a"]]]);
    const html = rows(s, keepDone(inbox(s), s, null, held("b", 0)), null);
    expect(html).toContain('<section aria-labelledby="group-done" data-leaving="true" inert="">');
    expect(html).toContain('Done<span class="group-n">0</span>');
  });
});
