// What the Schedules page says of the daemon's schedules and their runs,
// and the add it sends: pure, so the tests read it without a page.
import type {
  AddSchedule,
  HarnessChoice,
  RunOutcome,
  Schedule,
  ScheduleRun,
  ScheduleTarget,
  Schedules,
  ScheduleWhen,
  Weekday,
} from "./api/contract";
import type { Tone } from "./model";

export const DAYS: Weekday[] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
export const WEEKDAYS: Weekday[] = ["mon", "tue", "wed", "thu", "fri"];
const DAY_TITLE: Record<Weekday, string> = {
  mon: "Mon",
  tue: "Tue",
  wed: "Wed",
  thu: "Thu",
  fri: "Fri",
  sat: "Sat",
  sun: "Sun",
};

/// The orchestrator's rank of the focus picks each weekday at 07:30: the
/// one-click add while no rank is scheduled.
export const MORNING_RANK: AddSchedule = {
  name: "Morning rank",
  task: "rank",
  projectId: null,
  prompt: null,
  harness: null,
  model: null,
  effort: null,
  permissionMode: null,
  target: null,
  when: { kind: "at", hour: 7, minute: 30, days: WEEKDAYS },
};

export const TARGET_TITLE: Record<ScheduleTarget, string> = {
  newWorktree: "A new session in a new worktree",
  root: "A new session in the root checkout",
  session: "One session, each run typed into it",
  freshSession: "One session, each run on a new conversation",
};

const pad = (n: number) => String(n).padStart(2, "0");

export function clock(hour: number, minute: number): string {
  return `${pad(hour)}:${pad(minute)}`;
}

/// The days as a list says them: `daily`, `weekdays`, `weekends`, or
/// `Mon, Wed, Fri`.
export function daysLabel(days: Weekday[]): string {
  const set = new Set(days);
  const is = (want: Weekday[]) => want.length === set.size && want.every((d) => set.has(d));
  if (is(DAYS)) return "daily";
  if (is(WEEKDAYS)) return "weekdays";
  if (is(["sat", "sun"])) return "weekends";
  return DAYS.filter((d) => set.has(d))
    .map((d) => DAY_TITLE[d])
    .join(", ");
}

const startOfDay = (at: number) => new Date(at).setHours(0, 0, 0, 0);

/// `at` as a list says it from `now`: `today 07:30`, `tomorrow 07:30`,
/// `yesterday 07:30`, or `Tue 18 Nov 07:30`.
export function dayTime(at: number, now: number): string {
  const d = new Date(at);
  const time = clock(d.getHours(), d.getMinutes());
  const days = Math.round((startOfDay(at) - startOfDay(now)) / 86_400_000);
  if (days === 0) return `today ${time}`;
  if (days === 1) return `tomorrow ${time}`;
  if (days === -1) return `yesterday ${time}`;
  const day = d.toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" }).replace(",", "");
  return `${day} ${time}`;
}

/// When a schedule runs, as its row says it.
export function whenLabel(when: ScheduleWhen, now: number): string {
  switch (when.kind) {
    case "every":
      return when.minutes % 60 === 0 ? `every ${when.minutes / 60} h` : `every ${when.minutes} min`;
    case "at":
      return `${daysLabel(when.days)} ${clock(when.hour, when.minute)}`;
    case "once":
      return `once, ${dayTime(when.at, now)}`;
  }
}

/// When it runs next, or that it is done.
export function nextLabel(s: Schedule, now: number): string {
  return s.nextAt === null ? "done" : `next ${dayTime(s.nextAt, now)}`;
}

/// When a schedule runs, as its row in the list says it above its next run.
export function scheduleLine(s: Schedule, now: number): string {
  return s.when.kind === "once" ? "once" : whenLabel(s.when, now);
}

export function outcomeTone(outcome: RunOutcome): Tone {
  switch (outcome) {
    case "started":
    case "typed":
    case "restarted":
      return "green";
    case "held":
      return "yellow";
    case "failed":
      return "red";
    case "skipped":
      return "hollow";
  }
}

export const OUTCOME_WORD: Record<RunOutcome, string> = {
  started: "Started",
  held: "Held",
  typed: "Typed",
  restarted: "Restarted",
  skipped: "Skipped",
  failed: "Failed",
};

/// What a run did, past its word: what the run list says before the link
/// to the session it went to, or why it was skipped or failed.
export function outcomeSaid(run: ScheduleRun, rank: boolean): string {
  switch (run.outcome) {
    case "started":
      return "a new session";
    case "held":
      return rank
        ? "the rank turn, until the orchestrator is at rest with an empty input box"
        : "until its session is at rest with an empty input box";
    case "typed":
      return rank ? "the rank turn into the orchestrator" : "into its session";
    case "restarted":
      return "its session on a new conversation";
    case "skipped":
    case "failed":
      return run.why ?? "";
  }
}

/// The latest run of each schedule.
export function lastRuns(runs: ScheduleRun[]): Map<string, ScheduleRun> {
  const last = new Map<string, ScheduleRun>();
  for (const r of runs) if (!last.has(r.scheduleId)) last.set(r.scheduleId, r);
  return last;
}

export function hasRank(schedules: Schedule[]): boolean {
  return schedules.some((s) => s.task === "rank");
}

/// What the add form holds.
export interface Draft {
  task: "prompt" | "rank";
  name: string;
  projectId: string;
  prompt: string;
  target: ScheduleTarget;
  harness: string;
  model: string;
  effort: string;
  permissionMode: string;
  whenKind: ScheduleWhen["kind"];
  minutes: string;
  time: string;
  days: Weekday[];
  /// A `datetime-local` value, `2026-11-20T15:00`.
  once: string;
}

export function blankDraft(projectId: string): Draft {
  return {
    task: "prompt",
    name: "",
    projectId,
    prompt: "",
    target: "root",
    harness: "claude",
    model: "",
    effort: "",
    permissionMode: "",
    whenKind: "at",
    minutes: "60",
    time: "09:00",
    days: WEEKDAYS,
    once: "",
  };
}

/// Why a target is not to be had on `harness`, as the daemon would refuse
/// it; null when it is.
export function targetRefused(target: ScheduleTarget, harness: HarnessChoice | undefined): string | null {
  if (!harness || (target !== "session" && target !== "freshSession")) return null;
  if (!harness.reportsStatus) return `${harness.label} reports no status, so its session never reads as at rest.`;
  if (target === "session" && !harness.resumes) return `${harness.label} cannot resume a conversation.`;
  return null;
}

/// The add `d` asks for, or why it is not one yet.
export function addOf(d: Draft, now: number): AddSchedule | string {
  let when: ScheduleWhen;
  switch (d.whenKind) {
    case "every": {
      const minutes = Number(d.minutes);
      if (!Number.isInteger(minutes) || minutes < 1) return "Say how many minutes apart, 1 or more.";
      when = { kind: "every", minutes };
      break;
    }
    case "at": {
      const m = /^(\d{1,2}):(\d{2})$/.exec(d.time);
      if (!m) return "Say the time of day it runs at.";
      if (d.days.length === 0) return "Pick at least one day.";
      when = { kind: "at", hour: Number(m[1]), minute: Number(m[2]), days: DAYS.filter((x) => d.days.includes(x)) };
      break;
    }
    case "once": {
      const at = d.once ? new Date(d.once).getTime() : NaN;
      if (Number.isNaN(at)) return "Say the day and time it runs.";
      if (at <= now) return "That time has passed.";
      when = { kind: "once", at };
      break;
    }
  }
  if (d.task === "rank") return { ...MORNING_RANK, name: d.name.trim(), when };
  if (!d.projectId) return "Pick the project it runs in.";
  if (!d.prompt.trim()) return "Write the prompt each run starts on.";
  const or = (v: string) => (v.trim() === "" ? null : v.trim());
  return {
    name: d.name.trim(),
    task: "prompt",
    projectId: d.projectId,
    prompt: d.prompt.trim(),
    harness: d.harness,
    model: or(d.model),
    effort: or(d.effort),
    permissionMode: d.harness === "claude" ? or(d.permissionMode) : null,
    target: d.target,
    when,
  };
}

/// A daemon's schedules with none in them, for a state with nothing to say.
export const NO_SCHEDULES = {
  enabled: true,
  available: true,
  schedules: [],
  runs: [],
  harnesses: [],
  permissionModes: [],
  loginItem: null,
} satisfies Schedules;
