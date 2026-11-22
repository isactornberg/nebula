import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { watchScreen } from "./api/client";
import type { Screen, Session, SessionDetail, State, Turns } from "./api/contract";
import { Detail, type Layout } from "./Detail";
import { keyAction, pickFor } from "./model";
import { overlayOpen } from "./ui";
import { NO_SCHEDULES } from "./schedules";
import { Stick, Thread, threadItems, turnEnd } from "./Thread";

const MIN = 60_000;
const NOW = 100 * 86_400_000 + 11 * 3_600_000;

const session = (id: string, over: Partial<Session> = {}): Session => ({
  id,
  name: id,
  projectId: "p",
  projectName: "rover-firmware",
  branch: "main",
  rootCheckout: true,
  harness: "claude",
  role: "worker",
  status: "finished",
  statusChangedAt: NOW - 12 * MIN,
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
  server: { version: "test", protocol: 65 },
});

const detailOf = (s: Session, over: Partial<SessionDetail> = {}): SessionDetail => ({
  session: s,
  reply: { text: "The last reply.", source: "transcript", at: NOW - 12 * MIN },
  answers: [],
  dialog: null,
  workerIds: [],
  ...over,
});

const REPLY = [
  "Both workers reproduce the drop.",
  "",
  "| | udev rule | retry in code |",
  "|---|---|---|",
  "| Lines changed | 6 | 140 |",
  "",
  "QUESTION: keep the udev rule, or retry in code?",
  "NEXT: udev rule | retry in code",
].join("\n");

const turns: Turns = {
  turns: [
    { role: "user", text: "Find out why, as in this trace:\n[Image #1]", at: NOW - 58 * MIN },
    { role: "assistant", text: "Reproduced it.\n\nDONE started two workers\nNEXT: wait | stop", at: NOW - 52 * MIN },
    { role: "user", text: "How are the two doing?", at: NOW - 14 * MIN },
    { role: "assistant", text: REPLY, at: NOW - 12 * MIN },
  ],
  more: false,
};

function page(
  s: Session,
  over: {
    detail?: SessionDetail | null;
    turns?: Turns | null;
    layout?: Layout;
    terminal?: boolean;
    sessions?: Session[];
    canShow?: boolean;
  } = {},
) {
  return renderToStaticMarkup(
    <Detail
      state={state(over.sessions ?? [s])}
      session={s}
      detail={over.detail === undefined ? detailOf(s) : over.detail}
      turns={over.turns === undefined ? turns : over.turns}
      pick={null}
      note={null}
      onSay={null}
      focus={state([]).focus}
      now={NOW}
      busy={false}
      error={null}
      layout={over.layout ?? "wide"}
      back="Inbox"
      terminal={over.terminal ?? false}
      canShow={over.canShow ?? true}
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
    />,
  );
}

describe("a session's thread", () => {
  it("is its turns in order, under what the session is, its newest reply marked", () => {
    const s = session("cap", { role: "captain", issueKey: "ACME-900", issueUrl: "https://linear.app/a/issue/ACME-900" });
    const items = threadItems(s, detailOf(s), turns, []);
    expect(items.map((i) => i.kind)).toEqual(["start", "you", "reply", "you", "reply"]);
    expect(items[0]).toEqual({ kind: "start", text: "Captain for ACME-900 in rover-firmware, on main" });
    expect(items.flatMap((i) => (i.kind === "reply" ? [i.newest] : []))).toEqual([false, true]);
  });

  it("shows your prompts as yours, an image as a chip, and the replies as Markdown with their report", () => {
    const s = session("w");
    const html = renderToStaticMarkup(
      <Thread items={threadItems(s, detailOf(s), turns, [])} s={s} now={NOW} dropNext onOpen={() => {}} />,
    );
    expect(html.match(/class="you"/g)).toHaveLength(2);
    expect(html).toContain(
      '<div class="you-text">Find out why, as in this trace:\n<span class="you-image"><svg aria-hidden="true" class="icon" data-icon="image"',
    );
    expect(html).toMatch(/<table>[^]*<th>udev rule<\/th>[^]*<td>Lines changed<\/td>/);
    expect(html).toContain('<span class="tag tint-red">Question</span><span class="msg-report-text">Keep the udev rule, or retry in code?</span>');
    expect(html).toContain('<span class="tag tint-blue">Done</span><span class="msg-report-text">Started two workers</span>');
    expect(html).not.toContain("NEXT:");
    expect(html).toContain('<time class="t-time" dateTime="');
  });

  it("keeps the newest reply's NEXT line where its answers are not drawn", () => {
    const s = session("w");
    const html = renderToStaticMarkup(
      <Thread items={threadItems(s, detailOf(s), turns, [])} s={s} now={NOW} dropNext={false} onOpen={() => {}} />,
    );
    expect(html).toContain('<p class="msg-next">NEXT: udev rule | retry in code</p>');
    expect(html).not.toContain("NEXT: wait | stop");
  });

  it("says when the transcript holds older turns than it shows", () => {
    const s = session("w");
    const items = threadItems(s, detailOf(s), { ...turns, more: true }, []);
    expect(items[0]).toEqual({ kind: "more" });
    expect(page(s, { turns: { ...turns, more: true } })).toContain('<p class="t-rule">Earlier turns are not shown</p>');
    expect(page(s)).not.toContain("Earlier turns are not shown");
  });

  it("is the last reply alone for a session with no turns, and read off the screen for one that is not Claude's", () => {
    const s = session("w");
    for (const none of [{ turns: null, more: false }, null]) {
      const items = threadItems(s, detailOf(s), none, []);
      expect(items).toEqual([
        { kind: "start", text: "Session in rover-firmware, on main" },
        { kind: "reply", text: "The last reply.", at: NOW - 12 * MIN, newest: true, screen: false },
      ]);
    }
    const codex = session("c", { harness: "codex" });
    const screen = detailOf(codex, { reply: { text: "plain  text", source: "screen", at: null } });
    const html = page(codex, { detail: screen, turns: { turns: null, more: false } });
    expect(html).toContain('<span class="msg-who">Codex</span><span class="msg-role">read off the screen</span>');
    expect(html).toContain('<pre class="msg-plain">plain  text</pre>');
  });

  it("says what it waits for while nothing is read, and when there is nothing yet", () => {
    const fresh = session("f", { status: "fresh" });
    expect(page(fresh, { detail: null, turns: null })).toContain("Reading the conversation…");
    expect(page(fresh, { detail: detailOf(fresh, { reply: null }), turns: { turns: [], more: false } })).toContain(
      "Nothing here yet: the session has not started.",
    );
  });

  it("ends in the dialog the session shows, and in that it is at work", () => {
    const s = session("d", { status: "needsFeedback", why: "Bash: cargo build --release" });
    const dialog = { question: "Do you want to proceed?", options: [{ number: 1, label: "Yes" }, { number: 2, label: "No" }] };
    const items = threadItems(s, detailOf(s, { dialog }), turns, []);
    expect(items.at(-1)).toEqual({ kind: "dialog", dialog, why: "Bash: cargo build --release", at: s.statusChangedAt });
    const html = page(s, { detail: detailOf(s, { dialog }) });
    expect(html).toContain('<span class="tag tint-peach">Permission</span><span class="t-dialog-tool">Bash command</span>');
    expect(html).toContain('<div class="t-dialog-arg m">cargo build --release</div><p class="t-dialog-q">Do you want to proceed?</p>');

    const running = session("r", { status: "running" });
    expect(threadItems(running, detailOf(running), turns, []).at(-1)).toEqual({ kind: "working", at: running.statusChangedAt });
  });

  it("has a captain's workers in it by when each came to where it stands", () => {
    const cap = session("cap", { role: "captain" });
    const udev = session("udev", {
      name: "gps-udev",
      statusChangedAt: NOW - 8 * MIN,
      prUrl: "https://github.com/a/r/pull/31",
      prState: "open",
    });
    const retry = session("retry", { name: "gps-retry", status: "running", statusChangedAt: NOW - 30 * MIN });
    const items = threadItems(cap, detailOf(cap), turns, [udev, retry]);
    expect(items.map((i) => (i.kind === "worker" ? i.w.id : i.kind))).toEqual(["start", "you", "reply", "retry", "you", "reply", "udev"]);
    const html = page(cap, { detail: detailOf(cap, { workerIds: ["udev", "retry"] }), sessions: [cap, udev, retry] });
    expect(html).toMatch(/gps-udev<\/a> finished and opened <a href="https:\/\/github.com\/a\/r\/pull\/31" class="t-pr"[^>]*>PR #31<\/a>/);
    expect(html).toMatch(/gps-retry<\/a> is running/);
  });
});

describe("a session's head", () => {
  it("names the issue, the session, and the pull request with its state", () => {
    const s = session("w", {
      name: "Fix the login",
      rootCheckout: false,
      branch: "acme-7-fix",
      issueKey: "ACME-7",
      issueUrl: "https://linear.app/a/issue/ACME-7",
      issue: {
        key: "ACME-7",
        url: "https://linear.app/a/issue/ACME-7",
        title: "Login fails",
        priority: 2,
        state: "In Review",
        stateType: "started",
        assignee: null,
        branchName: "acme-7",
        updatedAt: NOW,
        team: "ACME",
        project: null,
        started: null,
        lastProject: null,
      },
      prUrl: "https://github.com/o/r/pull/9",
      prState: "merged",
    });
    const html = page(s);
    expect(html).toContain('<nav class="life" aria-label="Issue, session and pull requests">');
    expect(html).toMatch(/<span class="life-state">In Review<\/span>[^]*<span class="m life-key">ACME-7<\/span><span class="life-title">Login fails<\/span>/);
    expect(html).toMatch(/aria-current="step"[^]*<span class="life-name">Fix the login<\/span>/);
    expect(html).toMatch(/PR #9<span class="tag tint-mauve">merged<\/span>/);
    expect(html).toContain(">Open ACME-7 in Linear</span>");
    expect(html).toContain(">Open PR #9 on GitHub</span>");
  });

  it("names a worker's captain, and a captain's workers with their pull requests", () => {
    const cap = session("cap", { name: "GPS link recovery", role: "captain" });
    const udev = session("udev", { name: "gps-udev", prUrl: "https://github.com/a/r/pull/31", prState: "open" });
    expect(page(udev, { sessions: [cap, udev] })).toMatch(/worker of <a href="\/s\/cap">GPS link recovery<\/a>/);
    const html = page(cap, { detail: detailOf(cap, { workerIds: ["udev"] }), sessions: [cap, udev] });
    expect(html).toContain('<span class="cap-badge tint-mauve">');
    expect(html).toMatch(/<span>Workers<span class="life-and"> and PRs<\/span><\/span>[^]*<a class="life-worker" href="\/s\/udev">[^]*gps-udev<\/span><span class="tag tint-green">PR #31 ready<\/span>/);
  });

  it("offers Show in TUI while a TUI is open, the terminal on T, and Mark seen on an unseen finish", () => {
    const seen = page(session("w"));
    expect(seen).toContain('<span class="hl">Show in TUI</span>');
    expect(seen).toMatch(/aria-pressed="false" aria-keyshortcuts="t"[^>]*>[^]*<span class="hl">Terminal<\/span><kbd aria-hidden="true">T<\/kbd>/);
    expect(seen).not.toContain("Mark seen");
    expect(page(session("w"), { canShow: false })).not.toContain("Show in TUI");
    expect(page(session("w", { unseen: true }))).toContain('<span class="hl">Mark seen</span>');
  });

  it("on a phone, goes back to the list, and has the terminal and the rest in icons", () => {
    const html = page(session("w", { name: "GPS link recovery" }), { layout: "phone" });
    expect(html).toMatch(/<a class="sess-back" href="\/">[^]*Inbox<\/a>/);
    expect(html).toContain('aria-label="Live terminal"');
    expect(html).toContain('aria-label="More actions"');
    expect(html).toContain('<h1 class="sess-title">GPS link recovery</h1>');
  });
});

describe("answering", () => {
  it("draws the reply's answers with the keys 1-9 that pick them, and the dialog's options in their place", () => {
    const s = session("w");
    const html = page(s, { detail: detailOf(s, { answers: ["udev rule", "retry in code"] }) });
    expect(html).toContain('<div role="group" aria-label="Answers" class="dock-chips">');
    expect(html).toContain('aria-keyshortcuts="1"><kbd aria-hidden="true">1</kbd>udev rule</button>');
    expect(html).toContain('aria-keyshortcuts="2"><kbd aria-hidden="true">2</kbd>retry in code</button>');

    const d = session("d", { status: "needsFeedback" });
    const dialog = { question: "Proceed?", options: [{ number: 1, label: "Yes" }, { number: 2, label: "No" }] };
    const withDialog = page(d, { detail: detailOf(d, { dialog, answers: ["ignored"] }) });
    expect(withDialog).toContain('<div role="group" aria-label="Answer the dialog" class="dock-opts">');
    expect(withDialog).toContain('aria-keyshortcuts="2"><kbd aria-hidden="true">2</kbd><span>No</span></button>');
    expect(withDialog).not.toContain("ignored");
    expect(withDialog).toContain("A dialog is open: answer it first.");
  });

  it("picks answer or option n with key n", () => {
    const key = (k: string) => keyAction({ key: k, metaKey: false, ctrlKey: false, altKey: false });
    expect(key("2")).toEqual({ kind: "pick", n: 2 });
    expect(keyAction({ key: "2", metaKey: true, ctrlKey: false, altKey: false })).toBeNull();
    const s = session("w");
    expect(pickFor(2, detailOf(s, { answers: ["udev rule", "retry in code"] }))).toEqual({ kind: "answer", text: "retry in code" });
    expect(pickFor(3, detailOf(s, { answers: ["udev rule", "retry in code"] }))).toBeNull();
    const dialog = { question: "Proceed?", options: [{ number: 1, label: "Yes" }, { number: 2, label: "No" }] };
    expect(pickFor(2, detailOf(s, { dialog }))).toEqual({ kind: "option", option: 2 });
  });

  it("leaves the keys to a menu, a dialog or dictation open over the page", () => {
    const doc = (open: string[], popovers = true) => ({
      querySelector: (selectors: string) => {
        if (selectors === ":popover-open" && !popovers) throw new SyntaxError("not a selector");
        return selectors.split(",").some((s) => open.includes(s.trim())) ? ({} as Element) : null;
      },
    });
    expect(overlayOpen(doc([":popover-open"]))).toBe(true);
    expect(overlayOpen(doc(["dialog[open]"]))).toBe(true);
    expect(overlayOpen(doc([".dict"]))).toBe(true);
    expect(overlayOpen(doc([]))).toBe(false);
    // A browser without popovers still has its dialogs.
    expect(overlayOpen(doc(["dialog[open]"], false))).toBe(true);
    expect(overlayOpen(doc([], false))).toBe(false);
  });

  it("says when a turn ends what it ended on, as the working row goes", () => {
    expect(turnEnd(session("w"))).toBe("Replied");
    expect(turnEnd(session("w", { status: "needsFeedback" }))).toBe("A dialog is open");
    expect(turnEnd(session("w", { status: "terminated" }))).toBe("The session stopped");
    const html = renderToStaticMarkup(
      <Thread items={threadItems(session("w", { status: "running" }), null, turns, [])} s={session("w")} now={NOW} dropNext={false} onOpen={() => {}} />,
    );
    expect(html).toContain('<div class="thread"><p class="sr" role="status"></p>');
    expect(html).toContain('<p class="t-row working">');
  });

  it("has a composer with Image, the mic and Send, and on a phone the mic as its main button and Say it after the answers", () => {
    const s = session("w", { role: "captain" });
    const wide = page(s);
    expect(wide).toContain('<input type="file" accept="image/*" multiple="" hidden=""/>');
    expect(wide).toMatch(/<button type="button" class="compose-btn">[^]*Image<\/button>/);
    expect(wide).toContain('aria-label="Dictate a reply"');
    expect(wide).toMatch(/<button type="submit" class="send" disabled="">Send<kbd aria-hidden="true">↩<\/kbd><\/button>/);

    const phone = page(s, { layout: "phone", detail: detailOf(s, { answers: ["udev rule"] }) });
    expect(phone).toContain('<button type="button" class="round-btn mic" aria-label="Dictate a reply">');
    expect(phone).toContain('placeholder="Reply to the captain…"');
    expect(phone).toMatch(/class="dock-chip say tint-mauve">[^]*Say it<\/button>/);
    expect(phone).not.toContain('class="round-btn send"');
  });

  it("holds the composer while the session runs or has stopped", () => {
    expect(page(session("r", { status: "running" }))).toContain("Running: send once the turn is done.");
    expect(page(session("t", { status: "terminated" }))).toContain("The session has stopped and takes no follow-up.");
  });
});

describe("sticking to the bottom", () => {
  const box = (scrollTop: number, scrollHeight = 2000, clientHeight = 600) => ({ scrollTop, scrollHeight, clientHeight });

  it("opens at the bottom and stays there as turns come", () => {
    const stick = new Stick();
    const el = box(0);
    stick.grew(el);
    expect(el.scrollTop).toBe(2000);
    el.scrollTop = 1400;
    stick.scrolled(el);
    el.scrollHeight = 2600;
    stick.grew(el);
    expect(el.scrollTop).toBe(2600);
  });

  it("stays at the bottom when WebKit clamps it to the top as the turns replace the reply", () => {
    const stick = new Stick();
    const el = box(0, 735, 379);
    stick.grew(el);
    stick.scrolled(el);
    el.scrollHeight = 1583;
    el.scrollTop = 0;
    stick.scrolled(el);
    stick.grew(el);
    expect(el.scrollTop).toBe(1583);
    el.scrollTop = 400;
    stick.scrolled(el);
    el.scrollHeight = 2000;
    stick.grew(el);
    expect(el.scrollTop).toBe(400);
  });

  it("stays at the bottom as the on-screen keyboard shrinks the scroller and scrolls it", () => {
    // An iPhone 17e's Home Screen app, iOS 26.5: the keyboard takes the
    // thread from 592 px to 215, and a scroll from 234 to 208 comes with it.
    const stick = new Stick();
    const el = box(0, 826, 592);
    stick.grew(el);
    el.scrollTop = 234;
    stick.scrolled(el);
    el.clientHeight = 215;
    el.scrollTop = 208;
    stick.scrolled(el);
    stick.grew(el);
    expect(el.scrollTop).toBe(826);
  });

  it("leaves the view where it is once scrolled up, and sticks again back at the bottom", () => {
    const stick = new Stick();
    const el = box(0);
    stick.grew(el);
    el.scrollTop = 600;
    stick.scrolled(el);
    el.scrollHeight = 2600;
    stick.grew(el);
    expect(el.scrollTop).toBe(600);
    el.scrollTop = 1980;
    stick.scrolled(el);
    el.scrollHeight = 3000;
    stick.grew(el);
    expect(el.scrollTop).toBe(3000);
  });
});

describe("the live terminal", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("is shown beside the thread on a wide screen, in its place on a phone, and not at all while off", () => {
    const s = session("w");
    const wide = page(s, { terminal: true });
    expect(wide).toContain('<main class="sess" data-layout="wide" data-split="true">');
    expect(wide).toMatch(/<section class="sess-main"[^]*<\/section><section class="term" aria-label="Live terminal">/);
    expect(wide).toContain('class="thread"');
    expect(wide).toMatch(/aria-pressed="true" aria-keyshortcuts="t"/);
    expect(wide).not.toContain("Show in TUI");
    expect(wide).toContain(">Open in TUI</button>");

    const phone = page(s, { terminal: true, layout: "phone" });
    expect(phone).toContain('<section class="term" aria-label="Live terminal">');
    expect(phone).not.toContain('class="thread"');
    expect(phone).toContain('class="dock"');

    expect(page(s)).not.toContain("Live terminal");
  });

  it("reads the screen at once and every second while shown, and never once hidden", async () => {
    vi.useFakeTimers();
    const screen: Screen = { rows: ["❯ "], cols: 100, height: 40 };
    const fetched: string[] = [];
    const page = { hidden: false };
    vi.stubGlobal("document", page);
    vi.stubGlobal("fetch", (url: string) => {
      fetched.push(url);
      return Promise.resolve({ ok: true, json: () => Promise.resolve(screen) });
    });
    const got: Screen[] = [];
    const stop = watchScreen("w 1", (s) => got.push(s));
    expect(fetched).toEqual(["/api/sessions/w%201/screen"]);
    await vi.advanceTimersByTimeAsync(3000);
    expect(fetched).toHaveLength(4);
    expect(got).toHaveLength(4);
    page.hidden = true;
    await vi.advanceTimersByTimeAsync(5000);
    expect(fetched).toHaveLength(4);
    stop();
  });

  it("stops reading once stopped, and drops a read that answers after", async () => {
    vi.useFakeTimers();
    let answer: (r: unknown) => void = () => {};
    const fetched: string[] = [];
    vi.stubGlobal("document", { hidden: false });
    vi.stubGlobal("fetch", (url: string) => {
      fetched.push(url);
      return new Promise((done) => (answer = done));
    });
    const got: Screen[] = [];
    const stop = watchScreen("w", (s) => got.push(s));
    stop();
    answer({ ok: true, json: () => Promise.resolve({ rows: [], cols: 80, height: 24 }) });
    await vi.advanceTimersByTimeAsync(5000);
    expect(fetched).toHaveLength(1);
    expect(got).toEqual([]);
  });

  it("waits for a slow read to answer before the next, so reads never pile up or land out of order", async () => {
    vi.useFakeTimers();
    const answers: ((r: unknown) => void)[] = [];
    vi.stubGlobal("document", { hidden: false });
    vi.stubGlobal("fetch", () => new Promise((done) => answers.push(done)));
    const got: number[] = [];
    const stop = watchScreen("w", (s) => got.push(s.cols));
    await vi.advanceTimersByTimeAsync(4000);
    expect(answers).toHaveLength(1);
    answers[0]!({ ok: true, json: () => Promise.resolve({ rows: [], cols: 1, height: 24 }) });
    await vi.advanceTimersByTimeAsync(1000);
    expect(got).toEqual([1]);
    expect(answers).toHaveLength(2);
    stop();
  });

  it("skips a read while the page is out of sight", async () => {
    vi.useFakeTimers();
    const page = { hidden: true };
    const fetched: string[] = [];
    vi.stubGlobal("document", page);
    vi.stubGlobal("fetch", (url: string) => {
      fetched.push(url);
      return Promise.resolve({ ok: false });
    });
    const stop = watchScreen("w", () => {});
    await vi.advanceTimersByTimeAsync(2000);
    expect(fetched).toEqual([]);
    page.hidden = false;
    await vi.advanceTimersByTimeAsync(1000);
    expect(fetched).toHaveLength(1);
    stop();
  });
});
