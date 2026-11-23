import type { ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Schedule, Schedules, State, Today } from "./api/contract";
import { DayPills } from "./DeckPage";
import { ComingUp } from "./Idle";
import { NO_SCHEDULES } from "./schedules";
import { Tabs } from "./Shell";
import { chipOf, TodayChip, TodayTiles } from "./Today";
import allLate from "./testdata/all-late.json";
import javascriptLink from "./testdata/javascript-link.json";
import noNightly from "./testdata/no-nightly.json";
import noTraining from "./testdata/no-training.json";
import normal from "./testdata/normal.json";
import nothingDue from "./testdata/nothing-due.json";
import restDay from "./testdata/rest-day.json";
import trainingDone from "./testdata/training-done.json";
import trouble from "./testdata/trouble.json";

// Each day file under testdata, drawn on every surface the day shows on at
// one fixed time, pinned to a file under __snapshots__/day-parity: a new
// reader of the day must draw these the same. `pnpm test -u` writes them.
// The days' times are epoch ms, so the clock words need one zone.
process.env.TZ = "UTC";

// Friday 13 November 2026, 16:00.
const NOW = Date.UTC(2026, 10, 13, 16);

const DAYS: Record<string, Today & { version: number }> = {
  normal,
  trouble,
  "training-done": trainingDone,
  "no-training": noTraining,
  "rest-day": restDay,
  "no-nightly": noNightly,
  "nothing-due": nothingDue,
  "all-late": allLate,
  "javascript-link": javascriptLink,
};

/// What the server says of testdata/problem.json, which has no todos.
const PROBLEM = "The day in /home/you/.local/state/toolbox/today.json could not be read: missing field `todos`.";

const rank: Schedule = {
  id: "01rank",
  name: "Morning rank",
  task: "rank",
  projectId: null,
  projectName: null,
  prompt: null,
  harness: null,
  model: null,
  effort: null,
  permissionMode: null,
  target: null,
  when: { kind: "at", hour: 7, minute: 30, days: ["mon", "tue", "wed", "thu", "fri"] },
  nextAt: Date.UTC(2026, 10, 14, 7, 30),
  createdAt: 0,
  sessionId: null,
};

const go: Parameters<typeof Tabs>[0]["go"] = {
  section: "attention",
  view: "inbox",
  needsYou: 0,
  onView: () => {},
  onGo: () => {},
  making: false,
  schedules: true,
  onPalette: () => {},
  onPhones: () => {},
};

/// Each surface as App.tsx and DeckPage.tsx put it on the page, a tag a line.
function surfaces(today: Today | null, todayProblem: string | null): string {
  const state = { today, todayProblem, schedules: { ...NO_SCHEDULES, schedules: [rank] } as Schedules } as State;
  const chip = chipOf(state, NOW);
  const shown: [string, ReactNode][] = [
    ["Home band", <TodayTiles state={state} now={NOW} />],
    ["Today tab band", <TodayTiles state={state} now={NOW} head={false} />],
    ["Coming up", <ComingUp state={state} now={NOW} onGo={() => {}} />],
    ["List-foot chip", chip && <TodayChip chip={chip} onGo={() => {}} />],
    ["Phone tab bar", <Tabs go={go} todayTone={chip?.tone ?? null} />],
    ["Deck pills", today && <DayPills today={today} now={NOW} />],
  ];
  const drawn = shown.map(([name, node]) => `<!-- ${name} -->\n${renderToStaticMarkup(<>{node}</>).replaceAll("><", ">\n<")}`);
  return `${drawn.map((s) => s.trimEnd()).join("\n\n")}\n`;
}

describe("the day on every surface", () => {
  for (const [name, { version: _, ...today }] of Object.entries(DAYS)) {
    it(`draws ${name}`, async () => {
      await expect(surfaces(today, null)).toMatchFileSnapshot(`./__snapshots__/day-parity/${name}.html`);
    });
  }

  it("draws a file problem alone", async () => {
    await expect(surfaces(null, PROBLEM)).toMatchFileSnapshot("./__snapshots__/day-parity/problem.html");
  });
});
