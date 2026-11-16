// A dev-only stand-in for `nebula serve`: every endpoint of the contract,
// served by Vite from fixture data. Never part of the production build.
// `NEBULA_MOCK_FOCUS=rule pnpm dev` has the orchestrator not ranked yet, so
// the picks are by rule, `=stale` has its picks over three hours old with
// the board changed since, `=gone` picks no candidate of now, `=fewer` one
// issue, leaving every row that needs you out, and `=none` nothing.
// Re-rank, and opening the page on picks that are due, has the orchestrator
// rank a few seconds later, held while it works on a turn of yours;
// `NEBULA_MOCK_AUTO=off` keeps the page from asking on its own, and
// `NEBULA_MOCK_RANK=fail` has every rank end without picks.
// `NEBULA_MOCK_TUI=none pnpm dev` has no TUI open,
// `NEBULA_MOCK_BOARD=quiet pnpm dev` has nothing that needs you, and
// `NEBULA_MOCK_REFRESH=fail pnpm dev` has every refresh fail to read Linear
// and one project's pull requests.
// `NEBULA_MOCK_TODAY=none pnpm dev` has no day file, `=broken` one that
// is not a day, and `=trouble` a day whose todos and training could not be
// read, whose nightly run failed and whose kill switch is on. Start hands an
// issue to its project's captain, starting one in a project with none, holds
// the brief while that captain is at work, and fails to move ACME-962 in
// Linear.
// `NEBULA_MOCK_TAILNET=none pnpm dev` has no Tailscale running for Phones.
// `NEBULA_MOCK_SCHEDULES=rank pnpm dev` has the morning rank scheduled,
// `=none` no schedules and `=old` a daemon too old to keep any
// (`./schedules.ts`).
import type { IncomingMessage, ServerResponse } from "node:http";
import type { Plugin } from "vite";
import type {
  Attention,
  Board,
  BoardRow,
  Focus,
  FocusPick,
  FocusRank,
  Issue,
  IssueLane,
  IssueLaneName,
  IssueStart,
  IssueStarted,
  PushSubscription,
  Refresh,
  Screen,
  SectionName,
  Session,
  SessionDetail,
  Shown,
  State,
} from "../api/contract.ts";
import { MockDevices } from "./devices.ts";
import { issues as assigned, picks, projects, reranked, session, sessions as fixture, today, type MockSession } from "./fixtures.ts";
import { MockSchedules } from "./schedules.ts";
import { checkSkill, compareCopies, deleteSkill, fixSkills, saveSkill, skillDetail, skillList, skillUsage, switchPlugin, switchSkills, undoFix } from "./skills.ts";

const TURN_MS = 5000;
/// How long a refresh takes: long enough to see it run.
const REFRESH_MS = 1200;
/// How long a captain takes to start a worker on a brief: long enough to
/// Start a second issue while it works, whose brief is then held.
const CAPTAIN_TURN_MS = 15_000;
/// The issue whose move to its started state fails.
const MOVE_FAILS = "ACME-962";
/// How long the orchestrator takes to rank.
const RANK_MS = 4000;
const HOUR = 3_600_000;

export function mockApi(): Plugin {
  return {
    name: "nebula-mock-api",
    apply: "serve",
    configureServer(server) {
      const mock = new Mock(
        process.env.NEBULA_MOCK_FOCUS ?? "",
        process.env.NEBULA_MOCK_AUTO !== "off",
        process.env.NEBULA_MOCK_RANK === "fail",
        process.env.NEBULA_MOCK_TUI !== "none",
        process.env.NEBULA_MOCK_BOARD === "quiet",
        process.env.NEBULA_MOCK_REFRESH === "fail",
        process.env.NEBULA_MOCK_TODAY ?? "",
        process.env.NEBULA_MOCK_SCHEDULES ?? "",
      );
      const devices = new MockDevices(process.env.NEBULA_MOCK_TAILNET !== "none", Date.now());
      server.middlewares.use((req, res, next) => {
        if (!req.url?.startsWith("/api/")) return next();
        const path = new URL(req.url, "http://mock").pathname;
        const post = req.method === "POST";
        if (path === "/api/devices" && req.method === "GET") return reply(res, 200, devices.list(Date.now()));
        if (path === "/api/devices/pair" && post) {
          const answer = devices.pair(Date.now());
          return reply(res, answer.status, answer.body);
        }
        const remove = /^\/api\/devices\/([^/]+)\/remove$/.exec(path);
        if (remove?.[1] && post) {
          const answer = devices.remove(decodeURIComponent(remove[1]), Date.now());
          return reply(res, answer.status, answer.body);
        }
        const test = /^\/api\/devices\/([^/]+)\/test$/.exec(path);
        if (test?.[1] && post) {
          const answer = devices.test(decodeURIComponent(test[1]));
          return reply(res, answer.status, answer.body);
        }
        // A paired phone's page: the Mac's own is refused these.
        if (path.startsWith("/api/push")) {
          const phone = !["localhost", "127.0.0.1"].includes(new URL(`http://${req.headers.host ?? "localhost"}`).hostname);
          if (!phone)
            return reply(res, 404, {
              error: "Notifications are for a phone paired to this Mac: turn them on in nebula's page on it.",
            });
          if (path === "/api/push" && req.method === "GET") return reply(res, 200, devices.push());
          if (path === "/api/push/unsubscribe" && post) return reply(res, 200, devices.unsubscribe());
          if (path === "/api/push/subscribe" && post)
            return void readJson(req).then((sub) => {
              const answer = devices.subscribe(sub as Partial<PushSubscription> | null);
              reply(res, answer.status, answer.body);
            });
        }
        mock.handle(req, res).catch((e: unknown) => reply(res, 500, { error: String(e) }));
      });
    },
  };
}

function reply(res: ServerResponse, status: number, body?: unknown) {
  res.statusCode = status;
  if (body === undefined) return res.end();
  res.setHeader("Content-Type", "application/json");
  res.end(JSON.stringify(body));
}

async function readJson(req: IncomingMessage): Promise<Record<string, unknown>> {
  let raw = "";
  for await (const chunk of req) raw += String(chunk);
  try {
    const v: unknown = JSON.parse(raw || "{}");
    return v && typeof v === "object" ? (v as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

class Mock {
  sessions: MockSession[] = fixture(Date.now());
  tui: string | null = "rover-cap";
  streams = new Set<ServerResponse>();
  refresh: Refresh = { running: false, at: null, problems: [] };
  /// What the last refresh read: the issues assigned to you, and whether
  /// the sessions' pull requests were read.
  issues: Issue[] = [];
  prsRead = false;
  /// What Start did, by issue key; the project it last went to, by Linear
  /// project; the states it moved issues to; and the briefs it holds for
  /// captains at work, oldest first.
  handed = new Map<string, Omit<IssueStart, "held">>();
  lastProject = new Map<string, string>();
  moved = new Map<string, { state: string; stateType: string }>();
  briefs: { key: string; captainId: string }[] = [];
  /// The orchestrator's picks, the candidates it ranked them from (null:
  /// the ones there are), what you said of picks, the rank asked for last
  /// and where the daemon has it, and the ranks the page asked for itself.
  picks: { at: number; picks: { key: string; reason: string }[]; ranked: string[] | null } | null;
  said: { key: string; verdict: "notToday" | "wrong"; reason: string | null; at: number; picked: string; what: string }[] = [];
  asked: { at: number; auto: boolean } | null = null;
  rankHeldAt: number | null = null;
  rankTypedAt: number | null = null;
  runs: number[] = [];
  schedules: MockSchedules;

  constructor(
    scene: string,
    readonly autoRanks: boolean,
    readonly rankFails: boolean,
    readonly tuiOpen: boolean,
    quiet: boolean,
    readonly refreshFails: boolean,
    readonly day: string,
    schedules: string,
  ) {
    this.schedules = new MockSchedules(
      Date.now(),
      schedules,
      (id) => projects.find((p) => p.id === id)?.name ?? null,
      (name, projectId) => {
        const id = `sched-${this.sessions.length}`;
        this.sessions.push(
          session(Date.now(), { id, name, projectId, status: "running", ago: 0, branch: "main", rootCheckout: true }),
        );
        return id;
      },
      () => this.push(),
    );
    if (quiet) {
      this.sessions = this.sessions.filter((s) => !focusable(sectionOf(s)));
      this.tui = null;
    }
    const orch = this.sessions.find((s) => s.id === "orch")!;
    this.picks =
      scene === "rule"
        ? null
        : {
            at: scene === "stale" ? Date.now() - 4 * HOUR - 12 * 60_000 : orch.statusChangedAt,
            picks: picks(scene),
            ranked: scene === "stale" ? [] : null,
          };
  }

  async handle(req: IncomingMessage, res: ServerResponse) {
    const url = new URL(req.url!, "http://mock");
    const path = url.pathname;
    const get = req.method === "GET";
    if (get && path === "/api/state") return reply(res, 200, this.state());
    if (get && path === "/api/events") return this.stream(req, res);
    if (path === "/api/refresh") {
      if (req.method !== "POST") return reply(res, 405, { error: "Method not allowed." });
      this.startRefresh();
      return reply(res, 202);
    }
    if (path === "/api/focus/rank") {
      if (req.method !== "POST") return reply(res, 405, { error: "Method not allowed." });
      const answer = this.rank((await readJson(req)).auto === true);
      return reply(res, answer.status, answer.body);
    }
    if (path === "/api/focus/feedback") {
      if (req.method !== "POST") return reply(res, 405, { error: "Method not allowed." });
      const answer = this.feedback(await readJson(req));
      return reply(res, answer.status, answer.body);
    }
    if (get && path === "/api/attention") return reply(res, 200, this.attention());
    if (path === "/api/focus/picks") {
      if (req.method !== "POST") return reply(res, 405, { error: "Method not allowed." });
      let raw = "";
      for await (const chunk of req) raw += String(chunk);
      const answer = this.setPicks(raw);
      return reply(res, answer.status, answer.body);
    }
    if (path === "/api/schedules" && req.method === "POST") {
      const answer = this.schedules.add(await readJson(req), Date.now());
      return reply(res, answer.status, answer.body);
    }
    const scheduled = /^\/api\/schedules\/([^/]+)\/(remove|run)$/.exec(path);
    if (scheduled?.[1] && req.method === "POST") {
      const id = decodeURIComponent(scheduled[1]);
      const answer = scheduled[2] === "remove" ? this.schedules.remove(id) : this.schedules.run(id, Date.now());
      return reply(res, answer.status, answer.body);
    }
    const start = /^\/api\/issues\/([^/]+)\/start$/.exec(path);
    if (start?.[1]) {
      if (req.method !== "POST") return reply(res, 405, { error: "Method not allowed." });
      return this.startIssue(res, decodeURIComponent(start[1]), await readJson(req));
    }
    if (get && path === "/api/skills") return reply(res, 200, skillList());
    if (req.method === "POST" && (path === "/api/skills/switch" || path === "/api/skills/plugin")) {
      const body = await readJson(req);
      const answer = path === "/api/skills/switch" ? switchSkills(body) : switchPlugin(body);
      return reply(res, answer.status, answer.body);
    }
    if (req.method === "POST" && (path === "/api/skills/fix" || path === "/api/skills/undo")) {
      const body = await readJson(req);
      const answer = path === "/api/skills/fix" ? fixSkills(body) : undoFix(body);
      return reply(res, answer.status, answer.body);
    }
    if (req.method === "POST" && (path === "/api/skills/check" || path === "/api/skills/save")) {
      const body = await readJson(req);
      const answer = path === "/api/skills/check" ? checkSkill(body) : saveSkill(body);
      return reply(res, answer.status, answer.body);
    }
    if (req.method === "POST" && path === "/api/skills/delete") {
      const answer = deleteSkill(await readJson(req));
      return reply(res, answer.status, answer.body);
    }
    const compare = /^\/api\/skills\/compare\/([^/]+)$/.exec(path);
    if (get && compare?.[1]) {
      const answer = compareCopies(decodeURIComponent(compare[1]));
      return reply(res, answer.status, answer.body);
    }
    if (get && path === "/api/skills/usage") {
      // As slow as a first read of the transcripts, so the list is seen not to wait.
      await new Promise((r) => setTimeout(r, 600));
      return reply(res, 200, skillUsage(Date.now()));
    }
    const skill = /^\/api\/skills\/([^/]+)$/.exec(path);
    if (skill?.[1]) {
      if (!get) return reply(res, 405, { error: "Method not allowed." });
      const id = decodeURIComponent(skill[1]);
      const detail = skillDetail(id);
      return detail ? reply(res, 200, detail) : reply(res, 404, { error: `There is no skill ${id}.` });
    }
    if (req.method === "POST" && path === "/api/orchestrator/send") {
      const text = (await readJson(req)).text;
      if (typeof text !== "string" || !text.trim()) return reply(res, 400, { error: "Nothing to send." });
      return this.send(res, "orch", text);
    }
    const m = /^\/api\/sessions\/([^/]+)(?:\/(screen|send|answer|show|seen))?$/.exec(path);
    if (!m?.[1]) return reply(res, 404, { error: "No such endpoint." });
    const id = decodeURIComponent(m[1]);
    const s = this.sessions.find((x) => x.id === id);
    if (!s) return reply(res, 404, { error: `No session ${id}.` });
    const action = m[2];
    if (get && !action) return reply(res, 200, this.detail(s));
    if (get && action === "screen") return reply(res, 200, screenOf(s));
    if (req.method !== "POST") return reply(res, 405, { error: "Method not allowed." });
    if (action === "send") {
      const text = (await readJson(req)).text;
      if (typeof text !== "string" || !text.trim()) return reply(res, 400, { error: "Nothing to send." });
      return this.send(res, id, text);
    }
    if (action === "answer") {
      const option = (await readJson(req)).option;
      if (!s.dialog) return reply(res, 409, { error: "No dialog is open in the session." });
      const picked = s.dialog.options.find((o) => o.number === option);
      if (!picked) return reply(res, 409, { error: `The dialog has no option ${String(option)}.` });
      this.run(s, `Picked "${picked.label}" in the dialog.`);
      return reply(res, 202);
    }
    if (action === "show") {
      if (!this.tuiOpen) return reply(res, 409, { error: "No TUI is open to show the session in." });
      const shown: Shown = { alreadyOnScreen: this.tui === id };
      this.tui = id;
      s.unseen = false;
      this.push();
      return reply(res, 202, shown);
    }
    if (action === "seen") {
      s.unseen = false;
      this.push();
      return reply(res, 202);
    }
    return reply(res, 404, { error: "No such endpoint." });
  }

  send(res: ServerResponse, id: string, text: string) {
    const s = this.sessions.find((x) => x.id === id);
    if (!s) return reply(res, 404, { error: "There is no orchestrator yet." });
    if (s.status === "running") return reply(res, 409, { error: "The session is running." });
    if (s.dialog) return reply(res, 409, { error: "A dialog is open in the session." });
    if (s.status === "terminated" || s.status === "disconnected") {
      return reply(res, 409, { error: "The session has stopped." });
    }
    this.run(s, `You said: "${text.trim()}"`);
    return reply(res, 202);
  }

  /// Starts a turn on `s` that finishes after a few seconds with a reply.
  run(s: MockSession, said: string) {
    Object.assign(s, {
      status: "running",
      statusChangedAt: Date.now(),
      why: null,
      reportWord: null,
      dialog: null,
      answers: [],
      unseen: false,
    });
    this.push();
    setTimeout(() => {
      const report = "DONE: did what you asked";
      Object.assign(s, {
        status: "finished",
        statusChangedAt: Date.now(),
        unseen: true,
        why: "did what you asked",
        reply: `${said}\n\nThat is done, and the tests pass.\n\n${report}\nNEXT: open a PR | keep going`,
        replyAt: Date.now(),
        answers: ["open a PR", "keep going"],
      });
      this.push();
    }, TURN_MS);
  }

  /// Start, as the server makes it: the hand-off to the project's
  /// captain, then the move in Linear, which fails for one issue.
  async startIssue(res: ServerResponse, key: string, body: Record<string, unknown>) {
    const issue = this.issues.find((i) => i.key === key);
    if (!issue) {
      return reply(res, 404, { error: `${key} is not among the issues assigned to you that the last refresh read.` });
    }
    const project = projects.find((p) => p.id === body.project);
    if (!project) return reply(res, 404, { error: `There is no project ${String(body.project)}.` });
    if (project.team !== issue.team) {
      return reply(res, 409, { error: `${project.name} does not read its issues from Linear's ${issue.team} team.` });
    }
    const before = this.handed.get(key);
    if (before && body.again !== true) {
      return reply(res, 409, { error: `${key} was handed to the captain of ${before.projectName} already.` });
    }
    let captain = this.sessions.find((s) => s.role === "captain" && s.projectId === project.id);
    const fresh = !captain;
    if (!captain) {
      captain = {
        id: `${project.id}-cap`,
        name: "captain",
        projectId: project.id,
        projectName: project.name,
        branch: "main",
        rootCheckout: true,
        harness: "claude",
        role: "captain",
        status: "fresh",
        statusChangedAt: Date.now(),
        unseen: false,
        why: null,
        reportWord: null,
        issueUrl: null,
        issueKey: null,
        issue: null,
        prUrl: null,
        prState: null,
        reply: null,
        replyAt: null,
        answers: [],
        dialog: null,
      };
      this.sessions.push(captain);
    }
    const id = captain.id;
    const held =
      captain.status === "running" || captain.status === "needsFeedback" || this.briefs.some((b) => b.captainId === id);
    let movedTo: string | null = null;
    let moveFailed: string | null = null;
    if (issue.stateType !== "started") {
      if (key === MOVE_FAILS) {
        moveFailed = "Linear timed out.";
      } else {
        movedTo = "In Progress";
        this.moved.set(key, { state: movedTo, stateType: "started" });
        Object.assign(issue, this.moved.get(key));
      }
    }
    this.handed.set(key, { projectId: project.id, projectName: project.name, captainId: id, at: Date.now(), moveFailed });
    this.lastProject.set(`${issue.team}/${issue.project ?? ""}`, project.id);
    this.briefs.push({ key, captainId: id });
    if (!held) this.deliver(captain);
    // As slow as a captain coming up, or one found.
    await new Promise((r) => setTimeout(r, fresh ? 1500 : 500));
    this.push();
    const started: IssueStarted = { captainId: id, held, movedTo, moveFailed };
    return reply(res, 200, started);
  }

  /// Types the captain its oldest held brief: it works on it a while, then
  /// starts the worker, which carries the issue, and takes the next one.
  deliver(captain: MockSession) {
    const at = this.briefs.findIndex((b) => b.captainId === captain.id);
    if (at < 0) return;
    const [brief] = this.briefs.splice(at, 1);
    const issue = this.issues.find((i) => i.key === brief!.key);
    Object.assign(captain, { status: "running", statusChangedAt: Date.now(), why: null, reportWord: null, unseen: false });
    this.push();
    setTimeout(() => {
      if (issue) {
        const slug = issue.title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
        this.sessions.push({
          ...captain,
          id: `w-${issue.key.toLowerCase()}-${Date.now()}`,
          name: issue.title,
          branch: `${issue.key.toLowerCase()}-${slug}`.slice(0, 48),
          rootCheckout: false,
          role: "worker",
          status: "running",
          statusChangedAt: Date.now(),
          issueUrl: issue.url,
          issueKey: issue.key,
          reply: null,
          replyAt: null,
        });
      }
      Object.assign(captain, {
        status: "finished",
        statusChangedAt: Date.now(),
        unseen: true,
        why: `started ${issue?.title ?? "a worker"}`,
        reply: `I started a worker on ${issue?.key ?? "the issue"}, on a branch of its own.\n\nDONE started ${issue?.title ?? "a worker"}`,
        replyAt: Date.now(),
      });
      this.push();
      setTimeout(() => this.deliver(captain), 1000);
    }, CAPTAIN_TURN_MS);
  }

  /// The candidates, as the server finds them: the rows that need you or
  /// are done, by the issue joined to them or by session, then the lanes.
  candidates(sessions: Session[], board: Board): { key: string; lane: string; sessionId: string | null; what: string }[] {
    const off = new Set(
      this.said.filter((f) => f.verdict === "notToday" && f.at >= new Date().setHours(0, 0, 0, 0)).map((f) => f.key),
    );
    const found: { key: string; lane: string; sessionId: string | null; what: string }[] = [];
    for (const section of board.sections.filter((x) => focusable(x.section))) {
      for (const row of section.rows) {
        const s = sessions.find((x) => x.id === row.sessionId)!;
        const key = s.issue?.key ?? s.id;
        if (off.has(key) || found.some((f) => f.key === key)) continue;
        found.push({ key, lane: section.section, sessionId: s.id, what: s.issue ? `${s.issue.key} ${s.issue.title}` : s.name });
      }
    }
    for (const lane of board.lanes) {
      for (const i of lane.issues.filter((x) => !off.has(x.key))) {
        found.push({ key: i.key, lane: lane.lane, sessionId: null, what: `${i.key} ${i.title}` });
      }
    }
    return found;
  }

  /// The focus by the server's rules: the orchestrator's picks that are
  /// candidates still and not put aside, else the rule's, saying why.
  focus(sessions: Session[], board: Board): Focus {
    const now = Date.now();
    const found = this.candidates(sessions, board);
    const rank = this.rankState();
    const answers = (id: string) => this.sessions.find((s) => s.id === id)?.answers ?? [];
    const rule = (fallback: Focus["fallback"]): Focus => ({
      source: "rule",
      at: fallback && this.picks ? this.picks.at : null,
      picks: board.sections
        .filter((x) => focusable(x.section))
        .flatMap((x) => x.rows)
        .slice(0, 3)
        .map((r) => {
          const s = sessions.find((x) => x.id === r.sessionId)!;
          return { key: s.id, sessionId: s.id, reason: s.why ?? "", answers: answers(s.id) };
        }),
      fallback,
      rank,
    });
    const set = this.picks;
    if (!set) return rule(null);
    if (this.refresh.at === null) return rule("unread");
    const now_ = found.map((c) => `${c.key} ${c.lane}`).sort();
    const changed = set.ranked !== null && JSON.stringify([...set.ranked].sort()) !== JSON.stringify(now_);
    if (now - set.at > 3 * HOUR && changed) return rule("stale");
    const onBoard = set.picks.filter((p) => found.some((c) => c.key === p.key));
    if (onBoard.length === 0 && set.picks.length > 0) return rule("gone");
    const live = onBoard.filter((p) => !this.said.some((f) => f.key === p.key && f.at >= set.at));
    if (live.length === 0 && onBoard.length > 0) return rule("setAside");
    return {
      source: "orchestrator",
      at: set.at,
      picks: live.map((p): FocusPick => {
        const c = found.find((x) => x.key === p.key)!;
        return { key: p.key, sessionId: c.sessionId, reason: p.reason, answers: c.sessionId ? answers(c.sessionId) : [] };
      }),
      fallback: null,
      rank,
    };
  }

  rankState(): FocusRank | null {
    const asked = this.asked;
    if (!asked || (this.picks && this.picks.at >= asked.at)) return null;
    const orch = this.sessions.find((s) => s.id === "orch")!;
    const state: FocusRank["state"] =
      this.rankHeldAt !== null
        ? "held"
        : this.rankTypedAt !== null && orch.status !== "running" && orch.statusChangedAt > this.rankTypedAt
          ? "ended"
          : "ranking";
    return { at: asked.at, auto: asked.auto, state };
  }

  /// Re-rank, or the page asking as it opens: the orchestrator ranks once
  /// it is at rest, the rank held until then.
  rank(auto: boolean): { status: number; body?: unknown } {
    const rank = this.rankState();
    if (rank && rank.state !== "ended") return { status: 202 };
    const now = Date.now();
    const orch = this.sessions.find((s) => s.id === "orch")!;
    if (auto) {
      const today = new Date().setHours(0, 0, 0, 0);
      const set = this.picks;
      const stale = !set || (now - set.at > 3 * HOUR && set.ranked !== null);
      const busy = orch.status === "running" || orch.status === "needsFeedback";
      if (!this.autoRanks || !stale || busy || this.runs.filter((t) => t >= today).length >= 4) return { status: 204 };
      this.runs.push(now);
    }
    this.asked = { at: now, auto };
    this.rankHeldAt = now;
    this.push();
    const waiting = setInterval(() => {
      if (orch.status === "running" || orch.status === "needsFeedback") return;
      clearInterval(waiting);
      this.rankHeldAt = null;
      this.rankTypedAt = Date.now();
      Object.assign(orch, { status: "running", statusChangedAt: Date.now() });
      this.push();
      setTimeout(() => {
        if (!this.rankFails) {
          this.picks = { at: Date.now(), picks: reranked, ranked: null };
        }
        Object.assign(orch, {
          status: "finished",
          statusChangedAt: Date.now(),
          reply: this.rankFails ? "attention answered nothing I could rank." : "Picked the board PR, the rover captain's question and the OTA retry.",
          replyAt: Date.now(),
        });
        this.push();
      }, RANK_MS);
    }, 500);
    return { status: 202 };
  }

  feedback(body: Record<string, unknown>): { status: number; body?: unknown } {
    const sessions = this.sessions.map((s) => this.joined(s));
    const board = { ...boardOf(this.sessions), lanes: this.lanes(sessions) };
    const focus = this.focus(sessions, board);
    const pick = focus.source === "orchestrator" ? focus.picks.find((p) => p.key === body.key) : undefined;
    if (!pick) return { status: 404, body: { error: `${String(body.key)} is not among the orchestrator's picks.` } };
    if (body.verdict !== "notToday" && body.verdict !== "wrong") return { status: 400, body: { error: "No such verdict." } };
    const reason = typeof body.reason === "string" && body.reason.trim() ? body.reason.trim() : null;
    const what = this.candidates(sessions, board).find((c) => c.key === pick.key)?.what ?? pick.key;
    this.said.push({ key: pick.key, verdict: body.verdict, reason, at: Date.now(), picked: pick.reason, what });
    this.push();
    return { status: 202 };
  }

  attention(): Attention {
    const sessions = this.sessions.map((s) => this.joined(s));
    const board = { ...boardOf(this.sessions), lanes: this.lanes(sessions) };
    const issues = board.lanes.flatMap((l) => l.issues);
    const ago = (at: number) => {
      const m = Math.floor((Date.now() - at) / 60_000);
      return m < 1 ? "now" : m < 60 ? `${m}m` : m < 1440 ? `${Math.floor(m / 60)}h` : `${Math.floor(m / 1440)}d`;
    };
    const found = this.candidates(sessions, board);
    return {
      read: this.refresh.at === null ? null : ago(this.refresh.at),
      candidates: found.slice(0, 25).map((c) => {
        const s = c.sessionId ? sessions.find((x) => x.id === c.sessionId) : undefined;
        const i = s?.issue ?? issues.find((x) => x.key === c.key);
        return {
          key: c.key,
          lane: c.lane as Attention["candidates"][number]["lane"],
          ...(i && { issue: { title: i.title, state: i.state, changed: ago(i.updatedAt) } }),
          ...(s && {
            sessions: [
              { id: s.id, project: s.projectName, name: s.name, status: s.status, since: ago(s.statusChangedAt), ...(s.why && { why: s.why }) },
            ],
          }),
        };
      }),
      ...(found.length > 25 && { more: found.length - 25 }),
      feedback: this.said
        .filter((f) => Date.now() - f.at < 7 * 24 * HOUR)
        .reverse()
        .map((f) => ({ key: f.key, said: f.verdict, ...(f.reason && { reason: f.reason }), picked: f.picked, what: f.what, ago: ago(f.at) })),
    };
  }

  setPicks(raw: string): { status: number; body: unknown } {
    let got: unknown;
    try {
      got = JSON.parse(raw);
    } catch {
      return { status: 400, body: { error: 'The picks are not a JSON array of {"key", "reason"}.' } };
    }
    if (!Array.isArray(got)) return { status: 400, body: { error: 'The picks are not a JSON array of {"key", "reason"}.' } };
    if (got.length > 3) return { status: 400, body: { error: `At most three picks, not ${got.length}.` } };
    const sessions = this.sessions.map((s) => this.joined(s));
    const found = this.candidates(sessions, { ...boardOf(this.sessions), lanes: this.lanes(sessions) });
    const set: { key: string; reason: string }[] = [];
    for (const p of got as { key?: unknown; reason?: unknown }[]) {
      const key = String(p.key ?? "").trim();
      if (set.some((x) => x.key === key)) return { status: 400, body: { error: `${key} is picked twice.` } };
      if (!found.some((c) => c.key === key)) return { status: 400, body: { error: `${key} is no candidate.` } };
      const reason = String(p.reason ?? "").trim();
      if (!reason) return { status: 400, body: { error: `The pick of ${key} has no reason.` } };
      set.push({ key, reason });
    }
    this.picks = { at: Date.now(), picks: set, ranked: null };
    this.push();
    return { status: 200, body: { said: `Set ${set.length} focus picks.` } };
  }

  /// Starts a refresh, or joins the one under way.
  startRefresh() {
    if (this.refresh.running) return;
    this.refresh = { ...this.refresh, running: true };
    this.push();
    setTimeout(() => {
      const problems = this.refreshFails
        ? ["LINEAR_API_KEY is not set in nebula serve's environment.", "Pull requests in docs-site: gh timed out."]
        : [];
      if (!this.refreshFails) {
        this.issues = assigned(Date.now()).map((i) => ({ ...i, ...this.moved.get(i.key) }));
      }
      this.prsRead = true;
      this.refresh = { running: false, at: Date.now(), problems };
      this.push();
    }, REFRESH_MS);
  }

  /// `s` with the issue it works on and the pull request on its checkout,
  /// as the last refresh read them, by the server's rules: its issue URL's
  /// key, else the key in its branch as a whole word.
  joined(s: MockSession): MockSession {
    const inBranch = (key: string) => new RegExp(`(^|[^a-z0-9])${key.toLowerCase()}($|[^a-z0-9])`).test(s.branch);
    const issue = s.issueKey
      ? this.issues.find((i) => i.key === s.issueKey)
      : this.issues.find((i) => inBranch(i.key));
    const prRead = this.prsRead && !(this.refreshFails && s.projectId === "p-docs");
    return {
      ...s,
      issue: issue ?? null,
      prUrl: prRead ? s.prUrl : null,
      prState: prRead ? s.prState : null,
    };
  }

  /// The lanes of the issues no session works on.
  lanes(sessions: Session[]): IssueLane[] {
    const joined = new Set(sessions.flatMap((s) => (s.issue ? [s.issue.key] : [])));
    const rank = (i: Issue) => (i.priority === 0 ? 9 : i.priority);
    const lane = (name: IssueLaneName, stateType: string): IssueLane => ({
      lane: name,
      issues: this.issues
        .filter((i) => i.stateType === stateType && !joined.has(i.key))
        .sort((a, b) => rank(a) - rank(b) || b.updatedAt - a.updatedAt)
        .map((i) => {
          const handed = this.handed.get(i.key);
          return {
            ...i,
            started: handed ? { ...handed, held: this.briefs.some((b) => b.key === i.key) } : null,
            lastProject: this.lastProject.get(`${i.team}/${i.project ?? ""}`) ?? null,
          };
        }),
    });
    return [lane("inFlight", "started"), lane("todo", "unstarted")].filter((l) => l.issues.length);
  }

  stream(req: IncomingMessage, res: ServerResponse) {
    res.writeHead(200, {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-cache",
      Connection: "keep-alive",
    });
    this.streams.add(res);
    res.write(event(this.state()));
    const beat = setInterval(() => res.write(": keep-alive\n\n"), 15_000);
    req.on("close", () => {
      clearInterval(beat);
      this.streams.delete(res);
    });
  }

  push() {
    const e = event(this.state());
    for (const res of this.streams) res.write(e);
  }

  state(): State {
    const sessions = this.sessions.map((s) => {
      const { reply: _r, replyAt: _a, answers: _n, dialog: _d, ...session } = this.joined(s);
      return session;
    });
    const board = { ...boardOf(this.sessions), lanes: this.lanes(sessions) };
    return {
      projects,
      sessions,
      board,
      focus: this.focus(sessions, board),
      orchestratorId: "orch",
      tui: this.tuiOpen ? { sessionId: this.tui } : null,
      refresh: this.refresh,
      today: this.day === "none" || this.day === "broken" ? null : today(Date.now(), this.day === "trouble"),
      todayProblem:
        this.day === "broken"
          ? "The day in /home/you/.local/state/toolbox/today.json could not be read: missing field `todos`."
          : null,
      schedules: this.schedules.state(),
      server: { version: "0.0.0-mock", protocol: 1 },
    };
  }

  detail(s: MockSession): SessionDetail {
    const { reply, replyAt, answers, dialog, ...session } = this.joined(s);
    return {
      session,
      reply: reply === null ? null : { text: reply, source: "transcript", at: replyAt },
      answers,
      dialog,
      workerIds: s.role === "captain" ? workersOf(this.sessions, s).map((w) => w.id) : [],
    };
  }
}

function event(state: State): string {
  return `event: state\ndata: ${JSON.stringify(state)}\n\n`;
}

function sectionOf(s: MockSession): SectionName | null {
  if (s.status === "needsFeedback" || (s.status === "finished" && s.reportWord)) return "needsYou";
  if (s.status === "finished" && s.unseen) return "done";
  if (s.status === "running") return "running";
  return null;
}

const ORDER: SectionName[] = ["needsYou", "done", "running", "captains"];

/// The sections the rule picks focus from.
const focusable = (section: SectionName | null) => section === "needsYou" || section === "done";

function workersOf(all: MockSession[], captain: MockSession): MockSession[] {
  const rank = (s: MockSession) => ORDER.indexOf(sectionOf(s) ?? "captains");
  return all
    .filter((s) => s.projectId === captain.projectId && s.id !== captain.id && s.role === "worker")
    .sort((a, b) => rank(a) - rank(b) || b.statusChangedAt - a.statusChangedAt);
}

/// The board by the TUI's rules: a captain stands for its project, whose
/// other sessions are rows only while they need the user.
function boardOf(all: MockSession[]): Omit<Board, "lanes"> {
  const rows = new Map<SectionName, BoardRow[]>(ORDER.map((s) => [s, []]));
  const captains = new Map(all.filter((s) => s.role === "captain").map((c) => [c.projectId, c]));
  let idle = 0;
  let stopped = 0;
  const rest = (s: MockSession) => {
    if (s.status === "terminated" || s.status === "disconnected") stopped++;
    else if (s.status !== "running" && s.status !== "needsFeedback") idle++;
  };
  for (const s of [...all].sort((a, b) => b.statusChangedAt - a.statusChangedAt)) {
    if (s.role === "orchestrator") continue;
    const section = sectionOf(s);
    if (s.role === "captain") {
      const workers = workersOf(all, s).filter((w) => w.status === "running" || sectionOf(w) === "needsYou");
      rows.get(section ?? "captains")!.push({ sessionId: s.id, kind: "captain", workers: workers.length });
    } else if (section && (section === "needsYou" || !captains.has(s.projectId))) {
      rows.get(section)!.push({ sessionId: s.id, kind: "session", workers: 0 });
    } else {
      rest(s);
    }
  }
  return {
    sections: ORDER.map((section) => ({ section, rows: rows.get(section)! })).filter((s) => s.rows.length),
    idle,
    stopped,
  };
}

function screenOf(s: MockSession): Screen {
  const cols = 100;
  const wrap = (line: string) => {
    const out: string[] = [];
    for (let rest = line; ; rest = rest.slice(cols - 4)) {
      out.push(`  ${rest.slice(0, cols - 4)}`);
      if (rest.length <= cols - 4) return out;
    }
  };
  const rows = [` ✻ ${s.name}`, "", ...(s.reply ?? "").split("\n").flatMap(wrap), ""];
  if (s.dialog) {
    rows.push(` ╭${"─".repeat(cols - 4)}╮`, ` │ ${s.why ?? ""}`, ` │ ${s.dialog.question}`);
    s.dialog.options.forEach((o, i) => rows.push(` │ ${i === 0 ? "❯" : " "} ${o.number}. ${o.label}`));
    rows.push(` ╰${"─".repeat(cols - 4)}╯`);
  } else {
    if (s.status === "running") {
      const secs = Math.floor((Date.now() - s.statusChangedAt) / 1000);
      rows.push(` ✻ Working… (${secs}s · esc to interrupt)`, "");
    }
    rows.push("─".repeat(cols), " ❯ ", "─".repeat(cols), "   ⏵⏵ accept edits on (shift+tab to cycle)");
  }
  return { rows, cols, height: 40 };
}
