import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { HarnessChoice, Schedule, ScheduleRun, State } from "./api/contract";
import { parseRoute, routePath, sectionOf } from "./model";
import {
  addOf,
  blankDraft,
  dayTime,
  daysLabel,
  lastRuns,
  MORNING_RANK,
  nextLabel,
  NO_SCHEDULES,
  scheduleLine,
  targetRefused,
  whenLabel,
} from "./schedules";
import { SchedulesPage } from "./SchedulesPage";
import { ComingUp } from "./Idle";
import { More } from "./Shell";

// Monday 16 November 2026, 14:00 local time.
const NOW = new Date(2026, 10, 16, 14).getTime();
const at = (day: number, hour: number, minute = 0) => new Date(2026, 10, day, hour, minute).getTime();

const schedule = (over: Partial<Schedule> = {}): Schedule => ({
  id: "01kxa",
  name: "Overnight CI triage",
  task: "prompt",
  projectId: "p",
  projectName: "api",
  prompt: "Triage last night's failures.",
  harness: "claude",
  model: null,
  effort: null,
  permissionMode: null,
  target: "newWorktree",
  when: { kind: "at", hour: 6, minute: 0, days: ["mon", "tue", "wed", "thu", "fri", "sat", "sun"] },
  nextAt: at(17, 6),
  createdAt: at(10, 9),
  sessionId: null,
  ...over,
});

const run = (over: Partial<ScheduleRun> = {}): ScheduleRun => ({
  scheduleId: "01kxa",
  dueAt: at(16, 6),
  at: at(16, 6) + 2000,
  outcome: "started",
  sessionId: "s1",
  why: null,
  note: null,
  ...over,
});

const claude: HarnessChoice = {
  id: "claude",
  label: "Claude",
  models: [],
  efforts: [],
  reportsStatus: true,
  resumes: true,
};

describe("what the page says of a schedule", () => {
  it("says each form on the clock", () => {
    expect(whenLabel({ kind: "every", minutes: 30 }, NOW)).toBe("every 30 min");
    expect(whenLabel({ kind: "every", minutes: 120 }, NOW)).toBe("every 2 h");
    expect(whenLabel({ kind: "at", hour: 7, minute: 30, days: ["mon", "tue", "wed", "thu", "fri"] }, NOW)).toBe(
      "weekdays 07:30",
    );
    expect(whenLabel({ kind: "once", at: at(18, 15) }, NOW)).toBe("once, Wed 18 Nov 15:00");
    expect(daysLabel(["sun", "mon"])).toBe("Mon, Sun");
    expect(daysLabel(["sat", "sun"])).toBe("weekends");
  });

  it("says a time by its day from now", () => {
    expect(dayTime(at(16, 7, 5), NOW)).toBe("today 07:05");
    expect(dayTime(at(17, 6), NOW)).toBe("tomorrow 06:00");
    expect(dayTime(at(15, 6), NOW)).toBe("yesterday 06:00");
    expect(dayTime(at(13, 6), NOW)).toBe("Fri 13 Nov 06:00");
  });

  it("says a row's form above its next run, and when a once schedule is done", () => {
    expect(scheduleLine(schedule(), NOW)).toBe("daily 06:00");
    expect(nextLabel(schedule(), NOW)).toBe("next tomorrow 06:00");
    const once = schedule({ when: { kind: "once", at: at(18, 15) }, nextAt: at(18, 15) });
    expect(scheduleLine(once, NOW)).toBe("once");
    expect(nextLabel(once, NOW)).toBe("next Wed 18 Nov 15:00");
    expect(nextLabel({ ...once, nextAt: null }, NOW)).toBe("done");
  });

  it("knows each schedule's last run", () => {
    const last = lastRuns([run({ at: 3 }), run({ at: 2 }), run({ scheduleId: "b", at: 1 })]);
    expect(last.get("01kxa")?.at).toBe(3);
    expect(last.get("b")?.at).toBe(1);
  });
});

describe("the add form", () => {
  it("adds what it holds, the CLI's own model when none is named", () => {
    const d = { ...blankDraft("p"), prompt: "  Triage  ", time: "07:30", days: ["fri" as const, "mon" as const] };
    expect(addOf(d, NOW)).toEqual({
      name: "",
      task: "prompt",
      projectId: "p",
      prompt: "Triage",
      harness: "claude",
      model: null,
      effort: null,
      permissionMode: null,
      target: "root",
      when: { kind: "at", hour: 7, minute: 30, days: ["mon", "fri"] },
    });
    expect(addOf({ ...d, harness: "codex", permissionMode: "plan" }, NOW)).toMatchObject({ permissionMode: null });
    expect(addOf({ ...d, task: "rank" }, NOW)).toEqual({
      ...MORNING_RANK,
      name: "",
      when: { kind: "at", hour: 7, minute: 30, days: ["mon", "fri"] },
    });
  });

  it("says what is missing before it asks the daemon", () => {
    const d = { ...blankDraft("p"), prompt: "Triage" };
    expect(addOf({ ...d, prompt: " " }, NOW)).toBe("Write the prompt each run starts on.");
    expect(addOf({ ...d, projectId: "" }, NOW)).toBe("Pick the project it runs in.");
    expect(addOf({ ...d, days: [] }, NOW)).toBe("Pick at least one day.");
    expect(addOf({ ...d, whenKind: "every", minutes: "0" }, NOW)).toBe("Say how many minutes apart, 1 or more.");
    expect(addOf({ ...d, whenKind: "once", once: "2026-11-16T13:00" }, NOW)).toBe("That time has passed.");
    expect(addOf({ ...d, whenKind: "once", once: "2026-11-16T15:00" }, NOW)).toMatchObject({
      when: { kind: "once", at: at(16, 15) },
    });
  });

  it("refuses the same session where the harness cannot take it, as the daemon does", () => {
    expect(targetRefused("session", claude)).toBeNull();
    const muse = { ...claude, id: "muse", label: "Muse", reportsStatus: false, resumes: false };
    expect(targetRefused("freshSession", muse)).toBe("Muse reports no status, so its session never reads as at rest.");
    expect(targetRefused("root", muse)).toBeNull();
    const forgetful = { ...claude, label: "X", resumes: false };
    expect(targetRefused("session", forgetful)).toBe("X cannot resume a conversation.");
    expect(targetRefused("freshSession", forgetful)).toBeNull();
  });
});

describe("the Schedules routes", () => {
  it("open the list, the add form and one schedule", () => {
    for (const path of ["/schedules", "/schedules/new", "/schedules/01kxa"]) {
      const route = parseRoute(path);
      expect(sectionOf(route)).toBe("schedules");
      expect(routePath(route)).toBe(path);
    }
    expect(parseRoute("/schedules/new")).toEqual({ kind: "scheduleNew" });
  });
});

const state = (over: Partial<State["schedules"]> = {}, sessions: State["sessions"] = []): State =>
  ({
    projects: [{ id: "p", name: "api", space: null, team: null }],
    sessions,
    schedules: { ...NO_SCHEDULES, harnesses: [claude], ...over },
  }) as unknown as State;

const page = (s: State, view: Parameters<typeof SchedulesPage>[0]["view"]) =>
  renderToStaticMarkup(<SchedulesPage state={s} view={view} now={NOW} onGo={() => {}} />);

describe("the Schedules page", () => {
  it("offers the morning rank in one click while none is scheduled", () => {
    const none = page({ ...state(), orchestratorId: "o" }, { kind: "list" });
    expect(none).toMatch(/<button type="button" class="btn">Add the morning rank<\/button>/);
    expect(none).toContain("No schedules yet.");
    const alone = page(state(), { kind: "list" });
    expect(alone).toContain("No orchestrator runs: F10 in the TUI starts one.");
    expect(alone).toMatch(/<button type="button" class="btn" disabled="">Add the morning rank<\/button>/);
    const ranked = page(state({ schedules: [schedule({ task: "rank", name: "Morning rank" })] }), { kind: "list" });
    expect(ranked).not.toContain("Add the morning rank");
  });

  it("says to turn the login item on while it is off", () => {
    expect(page(state({ loginItem: false }), { kind: "list" })).toContain("nebula login-item on");
    expect(page(state({ loginItem: true }), { kind: "list" })).not.toContain("nebula login-item on");
  });

  it("lists each run, a skip with why, a late one with its note, and links the session a run started", () => {
    const red = { id: "s1", name: "Fix the build", status: "needsFeedback", reportWord: null } as State["sessions"][0];
    const html = page(
      state(
        {
          schedules: [schedule()],
          runs: [
            run({ note: "Caught up 2 h 12 min late" }),
            run({ outcome: "skipped", sessionId: null, why: "Missed: it was 1 day late", dueAt: at(15, 6) }),
          ],
        },
        [red],
      ),
      { kind: "one", id: "01kxa" },
    );
    expect(html).toContain("Overnight CI triage");
    expect(html).toContain('href="/s/s1"');
    // In the colour its row on the board has: a dialog's.
    expect(html).toContain('<span aria-hidden="true" class="sdot peach"></span>Fix the build</a>');
    expect(html).toContain("Caught up 2 h 12 min late.");
    expect(html).toContain("Missed: it was 1 day late");
    expect(html).toContain("Run now");
    expect(html).toContain("A new session in a new worktree");
    const list = page(state({ schedules: [schedule()], runs: [run()] }, [red]), { kind: "list" });
    expect(list).toContain('<span class="sched-dot" title="Its session: dialog"><span aria-hidden="true" class="sdot peach"></span></span>');
  });

  it("says a daemon too old to keep schedules is one", () => {
    expect(page(state({ available: false }), { kind: "list" })).toContain("The running daemon keeps no schedules");
  });

  it("says how to turn Schedules on while they are off, and offers nothing else", () => {
    const off = { enabled: false, available: false, loginItem: false };
    const html = page({ ...state(off), orchestratorId: "o" }, { kind: "list" });
    expect(html).toContain("Schedules are off: <code>&quot;schedules&quot;: true</code> in nebula&#x27;s <code>config.json</code>");
    expect(html).toContain("<code>nebula config path</code>");
    for (const gone of ["Add the morning rank", "nebula login-item on", "Recent runs", 'href="/schedules/new"', "still runs"])
      expect(html).not.toContain(gone);
    const kept = page(state({ ...off, schedules: [schedule()], runs: [run()] }), { kind: "list" });
    expect(kept).toContain("The daemon still runs the one it keeps: <code>nebula schedule list</code> lists them.");
    expect(kept).not.toContain("Overnight CI triage");
  });
});

describe("Schedules turned off", () => {
  const go = (schedules: boolean) => ({
    section: "attention" as const,
    view: "more" as const,
    needsYou: 0,
    onView: () => {},
    onGo: () => {},
    making: false,
    schedules,
    onPalette: () => {},
    onPhones: () => {},
  });
  const more = (on: boolean) => renderToStaticMarkup(<More go={go(on)} first={null} children={null} />);

  it("leave More with no link to them", () => {
    expect(more(true)).toContain('href="/schedules"');
    expect(more(false)).not.toContain('href="/schedules"');
  });

  it("leave Coming up with none of them", () => {
    const coming = (enabled: boolean) => {
      const book = { ...NO_SCHEDULES, enabled, available: enabled, schedules: [schedule({ nextAt: at(17, 6) })] };
      return renderToStaticMarkup(<ComingUp state={{ today: null, schedules: book } as unknown as State} now={NOW} onGo={() => {}} />);
    };
    expect(coming(true)).toContain('href="/schedules/01kxa"');
    expect(coming(false)).toBe("");
  });
});
