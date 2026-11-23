// The daemon's schedules and run log, as `nebula serve` serves them: an
// overnight CI triage whose run this morning stopped on a permission
// dialog (red on the board), with a day it was skipped and one it caught
// up; a same-session digest whose last run waits for its session; and a
// once run. No morning rank, so the page offers to add one;
// `NEBULA_MOCK_SCHEDULES=rank pnpm dev` has one, `=none` has no schedules,
// and `=old` a daemon too old to keep any. Run now on a same-session
// schedule holds its turn for a few seconds, then types it; on any other,
// starts a session; a rank holds and then types the rank turn.
import type {
  AddSchedule,
  HarnessChoice,
  RunOutcome,
  Schedule,
  ScheduleRun,
  Schedules,
} from "../api/contract.ts";

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;
const HELD_MS = 4000;

const HARNESSES: HarnessChoice[] = [
  {
    id: "claude",
    label: "Claude",
    models: ["fable", "opus", "sonnet", "haiku"],
    efforts: ["low", "medium", "high", "xhigh", "max"],
    reportsStatus: true,
    resumes: true,
  },
  { id: "codex", label: "Codex", models: ["gpt-5.6-sol", "gpt-5.5"], efforts: ["low", "medium", "high"], reportsStatus: true, resumes: true },
  { id: "cursor", label: "Cursor", models: [], efforts: [], reportsStatus: true, resumes: true },
  { id: "pi", label: "Pi", models: ["opus", "sonnet"], efforts: [], reportsStatus: true, resumes: true },
  { id: "muse", label: "Muse", models: [], efforts: [], reportsStatus: false, resumes: false },
  { id: "grok", label: "Grok Build", models: [], efforts: [], reportsStatus: false, resumes: true },
  { id: "opencode", label: "OpenCode", models: [], efforts: [], reportsStatus: true, resumes: true },
];

const MODES = ["manual", "acceptEdits", "auto", "plan", "dontAsk", "bypassPermissions"];

/// 07:30 on the day `at` is in, and `days` days after.
const at0730 = (at: number, days = 0) => new Date(at + days * DAY).setHours(7, 30, 0, 0);
const at0600 = (at: number, days = 0) => new Date(at + days * DAY).setHours(6, 0, 0, 0);

export class MockSchedules {
  schedules: Schedule[] = [];
  runs: ScheduleRun[] = [];
  private n = 0;

  constructor(
    now: number,
    readonly scene: string,
    readonly projectName: (id: string) => string | null,
    /// Starts a session for a run, its id.
    readonly startSession: (name: string, projectId: string) => string,
    readonly changed: () => void,
  ) {
    if (scene === "none" || scene === "old") return;
    const triage: Schedule = {
      id: "01kxtriage",
      name: "Overnight CI triage",
      task: "prompt",
      projectId: "p-nebula",
      projectName: "nebula",
      prompt:
        "Read last night's CI runs on main. For each failure, find the commit that broke it and start a worker to fix it. Report what you started.",
      harness: "claude",
      model: "opus",
      effort: "high",
      permissionMode: null,
      target: "newWorktree",
      when: { kind: "at", hour: 6, minute: 0, days: ["mon", "tue", "wed", "thu", "fri", "sat", "sun"] },
      nextAt: at0600(now, now >= at0600(now) ? 1 : 0),
      createdAt: now - 9 * DAY,
      sessionId: null,
    };
    const digest: Schedule = {
      id: "01kxdigest",
      name: "Shop's day so far",
      task: "prompt",
      projectId: "p-shop",
      projectName: "shopfront",
      prompt: "Say in five lines what changed in the shop since your last summary: merged PRs, open questions, what is blocked.",
      harness: "claude",
      model: "sonnet",
      effort: null,
      permissionMode: "plan",
      target: "session",
      when: { kind: "every", minutes: 120 },
      nextAt: now + 47 * MIN,
      createdAt: now - 3 * DAY,
      sessionId: "shop-cart",
    };
    const release: Schedule = {
      id: "01kxnotes",
      name: "Draft the release notes",
      task: "prompt",
      projectId: "p-docs",
      projectName: "docs-site",
      prompt: "Draft the release notes for the next docs release from the merged PRs since the last tag.",
      harness: "codex",
      model: null,
      effort: null,
      permissionMode: null,
      target: "root",
      when: { kind: "once", at: now + 2 * DAY + 3 * HOUR },
      nextAt: now + 2 * DAY + 3 * HOUR,
      createdAt: now - 2 * HOUR,
      sessionId: null,
    };
    this.schedules = [triage, digest, release];
    if (scene === "rank") {
      this.schedules.unshift({
        id: "01kxrank",
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
        nextAt: at0730(now, now >= at0730(now) ? 1 : 0),
        createdAt: now - 6 * DAY,
        sessionId: null,
      });
      this.runs.push({
        scheduleId: "01kxrank",
        dueAt: at0730(now, -1),
        at: at0730(now, -1) + 2000,
        outcome: "typed",
        sessionId: "orch",
        why: null,
        note: null,
      });
    }
    const run = (s: Schedule, dueAt: number, outcome: RunOutcome, over: Partial<ScheduleRun> = {}): ScheduleRun => ({
      scheduleId: s.id,
      dueAt,
      at: dueAt + 3000,
      outcome,
      sessionId: null,
      why: null,
      note: null,
      ...over,
    });
    this.runs.push(
      run(triage, at0600(now, -3), "started", { sessionId: "neb-e2e" }),
      run(triage, at0600(now, -2), "skipped", {
        at: at0600(now, -1) + 30 * MIN,
        why: "Missed: the daemon first saw it 1 day late, as the Mac slept or nebula's daemon was down, and a run more than ten minutes late is skipped",
      }),
      run(triage, at0600(now, -1), "started", {
        at: at0600(now, -1) + 2 * HOUR + 12 * MIN,
        sessionId: "neb-f8",
        note: "Caught up 2 h 12 min late, as the Mac slept or nebula's daemon was down: a daily schedule runs once late on the day it was due",
      }),
      run(triage, at0600(now), "started", { sessionId: "neb-dialog" }),
      run(digest, now - 5 * HOUR - 13 * MIN, "started", { sessionId: "shop-cart" }),
      run(digest, now - 3 * HOUR - 13 * MIN, "typed", { sessionId: "shop-cart" }),
      run(digest, now - HOUR - 13 * MIN, "failed", { why: "the session went before it took the turn" }),
    );
    this.runs.sort((a, b) => b.at - a.at);
  }

  state(): Schedules {
    return {
      enabled: this.scene !== "off",
      available: this.scene !== "old" && this.scene !== "off",
      schedules: this.scene === "old" ? [] : this.schedules,
      runs: this.runs,
      harnesses: HARNESSES,
      permissionModes: MODES,
      loginItem: false,
    };
  }

  add(body: Partial<AddSchedule>, now: number): { status: number; body: unknown } {
    if (this.scene === "old")
      return { status: 409, body: { error: "The running daemon keeps no schedules: `nebula reload` moves it onto this build, its sessions kept." } };
    const when = body.when;
    if (!when) return { status: 400, body: { error: "Say when it runs." } };
    if (when.kind === "once" && when.at <= now) return { status: 409, body: { error: "That time has passed: a once schedule runs at a time still to come." } };
    const rank = body.task === "rank";
    if (!rank) {
      if (!body.projectId) return { status: 400, body: { error: "Pick the project it runs in." } };
      if (!body.prompt?.trim()) return { status: 409, body: { error: "Message is empty." } };
      if (body.permissionMode && body.harness !== "claude")
        return { status: 409, body: { error: "A permission mode is Claude Code's --permission-mode." } };
    }
    const id = `01kxmock${++this.n}`;
    const name = body.name?.trim() || (rank ? "Morning rank" : body.prompt!.trim().split(/\s+/).slice(0, 6).join(" "));
    this.schedules.push({
      id,
      name,
      task: rank ? "rank" : "prompt",
      projectId: rank ? null : body.projectId!,
      projectName: rank ? null : this.projectName(body.projectId!),
      prompt: rank ? null : body.prompt!.trim(),
      harness: rank ? null : (body.harness ?? "claude"),
      model: body.model ?? null,
      effort: body.effort ?? null,
      permissionMode: body.permissionMode ?? null,
      target: rank ? null : (body.target ?? "root"),
      when,
      nextAt: nextOf(when, now),
      createdAt: now,
      sessionId: null,
    });
    this.changed();
    return { status: 200, body: { id } };
  }

  remove(id: string): { status: number; body?: unknown } {
    if (!this.schedules.some((s) => s.id === id)) return { status: 404, body: { error: `There is no schedule ${id}.` } };
    this.schedules = this.schedules.filter((s) => s.id !== id);
    this.runs = this.runs.filter((r) => r.scheduleId !== id);
    this.changed();
    return { status: 202 };
  }

  run(id: string, now: number): { status: number; body: unknown } {
    const s = this.schedules.find((x) => x.id === id);
    if (!s) return { status: 404, body: { error: `There is no schedule ${id}.` } };
    let outcome: RunOutcome = "started";
    let sessionId: string;
    if (s.task === "rank") {
      outcome = "held";
      sessionId = "orch";
    } else if ((s.target === "session" || s.target === "freshSession") && s.sessionId) {
      outcome = "held";
      sessionId = s.sessionId;
    } else {
      sessionId = this.startSession(s.name, s.projectId!);
      if (s.target === "session" || s.target === "freshSession") s.sessionId = sessionId;
    }
    const run: ScheduleRun = { scheduleId: id, dueAt: null, at: now, outcome, sessionId, why: null, note: null };
    this.runs.unshift(run);
    if (outcome === "held") {
      setTimeout(() => {
        run.outcome = s.target === "freshSession" ? "restarted" : "typed";
        this.changed();
      }, HELD_MS);
    }
    this.changed();
    return { status: 200, body: run };
  }
}

function nextOf(when: AddSchedule["when"], now: number): number | null {
  switch (when.kind) {
    case "every":
      return now + when.minutes * MIN;
    case "once":
      return when.at;
    case "at": {
      const days = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
      for (let d = 0; d < 8; d++) {
        const at = new Date(now + d * DAY).setHours(when.hour, when.minute, 0, 0);
        if (at > now && when.days.includes(days[new Date(at).getDay()] as never)) return at;
      }
      return null;
    }
  }
}
