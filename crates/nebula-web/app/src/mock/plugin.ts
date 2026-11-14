// A dev-only stand-in for `nebula serve`: every endpoint of the contract,
// served by Vite from fixture data. Never part of the production build.
// `NEBULA_MOCK_FOCUS=rule pnpm dev` serves the picks by rule,
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
import type { IncomingMessage, ServerResponse } from "node:http";
import type { Plugin } from "vite";
import type {
  Board,
  BoardRow,
  Issue,
  IssueLane,
  IssueLaneName,
  IssueStart,
  IssueStarted,
  Refresh,
  Screen,
  SectionName,
  Session,
  SessionDetail,
  Shown,
  State,
} from "../api/contract.ts";
import { issues as assigned, picks, projects, sessions as fixture, today, type MockSession } from "./fixtures.ts";
import { checkSkill, compareCopies, deleteSkill, fixSkills, saveSkill, skillDetail, skillList, skillUsage, switchPlugin, switchSkills, undoFix } from "./skills.ts";

const TURN_MS = 5000;
/// How long a refresh takes: long enough to see it run.
const REFRESH_MS = 1200;
/// How long a captain takes to start a worker on a brief: long enough to
/// Start a second issue while it works, whose brief is then held.
const CAPTAIN_TURN_MS = 15_000;
/// The issue whose move to its started state fails.
const MOVE_FAILS = "ACME-962";

export function mockApi(): Plugin {
  return {
    name: "nebula-mock-api",
    apply: "serve",
    configureServer(server) {
      const mock = new Mock(
        process.env.NEBULA_MOCK_FOCUS === "rule",
        process.env.NEBULA_MOCK_TUI !== "none",
        process.env.NEBULA_MOCK_BOARD === "quiet",
        process.env.NEBULA_MOCK_REFRESH === "fail",
        process.env.NEBULA_MOCK_TODAY ?? "",
      );
      server.middlewares.use((req, res, next) => {
        if (!req.url?.startsWith("/api/")) return next();
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

  constructor(
    readonly byRule: boolean,
    readonly tuiOpen: boolean,
    quiet: boolean,
    readonly refreshFails: boolean,
    readonly day: string,
  ) {
    if (quiet) {
      this.sessions = this.sessions.filter((s) => !focusable(sectionOf(s)));
      this.tui = null;
    }
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
    const board = boardOf(this.sessions);
    const live = new Set(this.sessions.map((s) => s.id));
    const focus = this.byRule
      ? {
          source: "rule" as const,
          at: null,
          picks: board.sections
            .filter((s) => focusable(s.section))
            .flatMap((s) => s.rows)
            .slice(0, 3)
            .map((r) => {
              const s = this.sessions.find((x) => x.id === r.sessionId)!;
              return { sessionId: s.id, reason: s.why ?? "", answers: s.answers };
            }),
        }
      : {
          source: "orchestrator" as const,
          at: this.sessions.find((s) => s.id === "orch")!.statusChangedAt,
          picks: picks
            .filter((p) => live.has(p.sessionId))
            .map((p) => ({ ...p, answers: this.sessions.find((s) => s.id === p.sessionId)!.answers })),
        };
    const sessions = this.sessions.map((s) => {
      const { reply: _r, replyAt: _a, answers: _n, dialog: _d, ...session } = this.joined(s);
      return session;
    });
    return {
      projects,
      sessions,
      board: { ...board, lanes: this.lanes(sessions) },
      focus,
      orchestratorId: "orch",
      tui: this.tuiOpen ? { sessionId: this.tui } : null,
      refresh: this.refresh,
      today: this.day === "none" || this.day === "broken" ? null : today(Date.now(), this.day === "trouble"),
      todayProblem:
        this.day === "broken"
          ? "The day in /home/you/.local/state/toolbox/today.json could not be read: missing field `todos`."
          : null,
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
