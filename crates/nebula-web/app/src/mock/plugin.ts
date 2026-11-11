// A dev-only stand-in for `nebula serve`: every endpoint of the contract,
// served by Vite from fixture data. Never part of the production build.
// `NEBULA_MOCK_FOCUS=rule pnpm dev` serves the picks by rule,
// `NEBULA_MOCK_TUI=none pnpm dev` has no TUI open, and
// `NEBULA_MOCK_BOARD=quiet pnpm dev` has nothing that needs you.
import type { IncomingMessage, ServerResponse } from "node:http";
import type { Plugin } from "vite";
import type { Board, BoardRow, Screen, SectionName, SessionDetail, Shown, State } from "../api/contract.ts";
import { picks, projects, sessions as fixture, type MockSession } from "./fixtures.ts";
import { compareCopies, fixSkills, skillDetail, skillList, skillUsage, switchPlugin, switchSkills, undoFix } from "./skills.ts";

const TURN_MS = 5000;

export function mockApi(): Plugin {
  return {
    name: "nebula-mock-api",
    apply: "serve",
    configureServer(server) {
      const mock = new Mock(
        process.env.NEBULA_MOCK_FOCUS === "rule",
        process.env.NEBULA_MOCK_TUI !== "none",
        process.env.NEBULA_MOCK_BOARD === "quiet",
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

  constructor(
    readonly byRule: boolean,
    readonly tuiOpen: boolean,
    quiet: boolean,
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
    return {
      projects,
      sessions: this.sessions.map(
        ({ reply: _r, replyAt: _a, answers: _n, dialog: _d, ...session }) => session,
      ),
      board,
      focus,
      orchestratorId: "orch",
      tui: this.tuiOpen ? { sessionId: this.tui } : null,
      today: null,
      server: { version: "0.0.0-mock", protocol: 1 },
    };
  }

  detail(s: MockSession): SessionDetail {
    const { reply, replyAt, answers, dialog, ...session } = s;
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
function boardOf(all: MockSession[]): Board {
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
