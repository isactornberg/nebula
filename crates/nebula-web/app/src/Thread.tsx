import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from "react";
import type { Dialog, Session, SessionDetail, Turns } from "./api/contract";
import { age, canSend, harnessName, prLabel, sessionIssue, splitReply, when, type ReportWord } from "./model";
import { External, Icon, Markdown, md, RouteLink, type IconName } from "./ui";

/// What a session's thread shows, oldest first.
export type ThreadItem =
  /// The line above the first turn shown: what the session is, or that
  /// older turns are left out.
  | { kind: "start"; text: string }
  | { kind: "more" }
  | { kind: "you"; text: string; at: number | null }
  | { kind: "reply"; text: string; at: number | null; newest: boolean; screen: boolean }
  /// A captain's worker, as it stands now, at the time it came to.
  | { kind: "worker"; w: Session }
  | { kind: "dialog"; dialog: Dialog; why: string | null; at: number }
  | { kind: "working"; at: number };

/// The thread of `s`: its turns when its transcript gave them, else its
/// last reply alone; a captain's workers by the time each came to where it
/// is; the dialog it shows; and that it is at work.
export function threadItems(s: Session, detail: SessionDetail | null, turns: Turns | null, workers: Session[]): ThreadItem[] {
  const said: ThreadItem[] = turns?.turns
    ? turns.turns.map((t) =>
        t.role === "user"
          ? { kind: "you", text: t.text, at: t.at }
          : { kind: "reply", text: t.text, at: t.at, newest: false, screen: false },
      )
    : detail?.reply
      ? [{ kind: "reply", text: detail.reply.text, at: detail.reply.at, newest: false, screen: detail.reply.source === "screen" }]
      : [];
  const last = said.findLastIndex((i) => i.kind === "reply");
  if (last >= 0) said[last] = { ...(said[last] as ThreadItem & { kind: "reply" }), newest: true };

  const rows = [...workers].sort((a, b) => a.statusChangedAt - b.statusChangedAt);
  const items: ThreadItem[] = [];
  let next = 0;
  for (const item of said) {
    const at = item.kind === "you" || item.kind === "reply" ? item.at : null;
    while (at !== null && next < rows.length && rows[next]!.statusChangedAt <= at) items.push({ kind: "worker", w: rows[next++]! });
    items.push(item);
  }
  for (const w of rows.slice(next)) items.push({ kind: "worker", w });
  if (detail?.dialog) items.push({ kind: "dialog", dialog: detail.dialog, why: s.why, at: s.statusChangedAt });
  if (s.status === "running") items.push({ kind: "working", at: s.statusChangedAt });
  if (items.length === 0) return [];
  return [turns?.more ? { kind: "more" } : { kind: "start", text: startLine(s) }, ...items];
}

/// `Captain for ACME-900 in rover-firmware, on main`.
function startLine(s: Session): string {
  if (s.role === "orchestrator") return "Orchestrator, across every project";
  const key = sessionIssue(s)?.key;
  return `${s.role === "captain" ? "Captain" : "Session"}${key ? ` for ${key}` : ""} in ${s.projectName}, on ${s.branch}`;
}

/// How far from the bottom still counts as at it.
const SLACK = 40;

type Scroller = { scrollTop: number; scrollHeight: number; clientHeight: number };

export function nearBottom(el: Scroller): boolean {
  return el.scrollHeight - el.scrollTop - el.clientHeight <= SLACK;
}

/// Keeps a scroller at its bottom as what it holds grows, until the user
/// scrolls up from it, and again once they scroll back down.
///
/// A scroll that comes with a change in how far the scroller can scroll is
/// not the user's: WebKit lays a thread out once before the turns that
/// replaced its reply are drawn, clamps it to the top, and says so in a
/// scroll, and the on-screen keyboard shrinking the scroller scrolls it too,
/// either of which would read as the user leaving the bottom. The growth
/// that follows puts it back.
export class Stick {
  stuck = true;
  room = 0;
  scrolled(el: Scroller) {
    const room = el.scrollHeight - el.clientHeight;
    if (room !== this.room) {
      this.room = room;
      return;
    }
    this.stuck = nearBottom(el);
  }
  grew(el: Scroller) {
    if (this.stuck) el.scrollTop = el.scrollHeight;
    this.room = el.scrollHeight - el.clientHeight;
  }
}

/// `scroller` opens at its bottom and stays there as `content` grows or
/// the scroller shrinks, while the user has not scrolled up.
export function useStick(scroller: RefObject<HTMLElement | null>, content: RefObject<HTMLElement | null>) {
  useLayoutEffect(() => {
    const el = scroller.current;
    const inner = content.current;
    if (!el || !inner) return;
    const stick = new Stick();
    stick.grew(el);
    const scrolled = () => stick.scrolled(el);
    el.addEventListener("scroll", scrolled, { passive: true });
    const grown = new ResizeObserver(() => stick.grew(el));
    grown.observe(inner);
    grown.observe(el);
    return () => {
      el.removeEventListener("scroll", scrolled);
      grown.disconnect();
    };
  }, [scroller, content]);
}

/// When something was, as the board says it, with the clock time on hover.
export function Time({ at, now }: { at: number; now: number }) {
  return (
    <time className="t-time" dateTime={new Date(at).toISOString()} title={when(at, now)}>
      {age(at, now)}
    </time>
  );
}

export function Thread({
  items,
  s,
  now,
  dropNext,
  onOpen,
}: {
  items: ThreadItem[];
  s: Session;
  now: number;
  /// The newest reply's answers are drawn as chips, so its NEXT line goes.
  dropNext: boolean;
  onOpen: (id: string) => void;
}) {
  const said = useTurnSaid(s, items.some((i) => i.kind === "working"));
  return (
    <div className="thread">
      <p className="sr" role="status">
        {said}
      </p>
      {items.map((item, i) => {
        switch (item.kind) {
          case "start":
            return (
              <p key={i} className="t-rule">
                {item.text}
              </p>
            );
          case "more":
            return (
              <p key={i} className="t-rule">
                Earlier turns are not shown
              </p>
            );
          case "you":
            return <You key={i} text={item.text} at={item.at} now={now} />;
          case "reply":
            return <Reply key={i} item={item} s={s} now={now} dropNext={dropNext} />;
          case "worker":
            return <WorkerRow key={item.w.id} w={item.w} now={now} onOpen={onOpen} />;
          case "dialog":
            return <DialogCard key={i} dialog={item.dialog} why={item.why} at={item.at} now={now} />;
          case "working":
            return (
              <p key={i} className="t-row working">
                <span aria-hidden="true" className="sdot green pulse" />
                <span className="t-row-text">{harnessName(s)} is at work</span>
                <Time at={item.at} now={now} />
              </p>
            );
        }
      })}
    </div>
  );
}

/// What a screen reader hears as a turn of `s` starts and ends, the
/// working row's coming and going: that it is at work, then what it ended
/// on. Nothing as the thread opens, or opens on another session.
function useTurnSaid(s: Session, working: boolean): string {
  const [said, setSaid] = useState("");
  const was = useRef({ id: s.id, working });
  useEffect(() => {
    const before = was.current;
    was.current = { id: s.id, working };
    if (before.id !== s.id) return setSaid("");
    if (before.working === working) return;
    setSaid(working ? `${harnessName(s)} is at work` : turnEnd(s));
  }, [s.id, working]);
  return said;
}

/// `Replied`, or why the turn ended without a reply to read.
export function turnEnd(s: Session): string {
  if (s.status === "needsFeedback") return "A dialog is open";
  return canSend(s) ? "Replied" : "The session stopped";
}

/// A prompt of yours, as typed, an image it carried as a chip.
function You({ text, at, now }: { text: string; at: number | null; now: number }) {
  const parts = text.split(/\[Image #(\d+)\]/);
  return (
    <div className="you">
      <span className="sr">You said:</span>
      <div className="you-text">
        {parts.map((p, i) =>
          i % 2 === 0 ? (
            p
          ) : (
            <span key={i} className="you-image">
              <Icon name="image" size={13} />
              Image {p}
            </span>
          ),
        )}
      </div>
      {at !== null && <Time at={at} now={now} />}
    </div>
  );
}

const REPORT_TINT: Record<ReportWord, string> = { QUESTION: "tint-red", BLOCKED: "tint-red", DONE: "tint-blue" };
const REPORT_NAME: Record<ReportWord, string> = { QUESTION: "Question", BLOCKED: "Blocked", DONE: "Done" };

const FACE: Record<Session["role"], [IconName, string]> = {
  worker: ["bot", "tint-blue"],
  captain: ["anchor", "tint-mauve"],
  orchestrator: ["sparkle", "tint-mauve"],
};

function Reply({
  item,
  s,
  now,
  dropNext,
}: {
  item: ThreadItem & { kind: "reply" };
  s: Session;
  now: number;
  dropNext: boolean;
}) {
  // An older reply's NEXT line offered answers that are gone.
  const drop = !item.newest || dropNext;
  const { body, report, next } = useMemo(() => splitReply(item.text, drop), [item.text, drop]);
  const [face, tint] = FACE[s.role];
  return (
    <article className="msg">
      <span aria-hidden="true" className={`msg-face ${tint}`}>
        <Icon name={face} size={15} />
      </span>
      <div className="msg-head">
        <span className="msg-who">{harnessName(s)}</span>
        <span className="msg-role">{item.screen ? "read off the screen" : s.role}</span>
        {item.at !== null && <Time at={item.at} now={now} />}
        {item.newest && s.unseen && <span className="tag tint-blue msg-unseen">Unseen</span>}
      </div>
      <div className="msg-body">
        {body && (item.screen ? <pre className="msg-plain">{body}</pre> : <Markdown text={body} />)}
        {report && (
          <p className={`msg-report report-${report.word.toLowerCase()}`}>
            <span className={`tag ${REPORT_TINT[report.word]}`}>{REPORT_NAME[report.word]}</span>
            <span
              className="msg-report-text"
              dangerouslySetInnerHTML={{ __html: md.renderInline(report.text.charAt(0).toUpperCase() + report.text.slice(1)) }}
            />
          </p>
        )}
        {next && <p className="msg-next">{next}</p>}
      </div>
    </article>
  );
}

/// A captain's worker in a line: where it stands and its pull request.
function WorkerRow({ w, now, onOpen }: { w: Session; now: number; onOpen: (id: string) => void }) {
  const [icon, tint, said] = workerSaid(w);
  const pr: ReactNode = w.prUrl && (
    <External href={w.prUrl} className="t-pr">
      {prLabel(w.prUrl)}
    </External>
  );
  const prSaid =
    w.prUrl && w.status === "finished" && !w.reportWord
      ? w.prState === "merged"
        ? <>, {pr} merged</>
        : w.prState === "closed"
          ? <>, {pr} closed</>
          : <> and opened {w.prState === "draft" ? "draft " : ""}{pr}</>
      : null;
  return (
    <p className="t-row">
      <span aria-hidden="true" className={`t-row-face ${tint}`}>
        <Icon name={icon} size={11} />
      </span>
      <span className="t-row-text">
        <RouteLink to={{ kind: "session", id: w.id }} onGo={() => onOpen(w.id)} className="m t-who">
          {w.name}
        </RouteLink>{" "}
        {said}
        {prSaid}
      </span>
      <Time at={w.statusChangedAt} now={now} />
    </p>
  );
}

function workerSaid(w: Session): [IconName, string, string] {
  switch (w.status) {
    case "running":
      return ["running", "tint-green", "is running"];
    case "needsFeedback":
      return ["shield", "tint-peach", "shows a dialog"];
    case "finished":
      if (w.reportWord === "QUESTION") return ["more", "tint-red", "asks a question"];
      if (w.reportWord === "BLOCKED") return ["close", "tint-red", "is blocked"];
      return ["check", w.prState === "merged" ? "tint-mauve" : "tint-green", "finished"];
    case "fresh":
      return ["play", "tint-plain", "started"];
    case "terminated":
      return ["close", "tint-plain", "stopped"];
    case "disconnected":
      return ["close", "tint-plain", "is disconnected"];
  }
}

/// `Bash: cargo build --release`, the board's line for a permission dialog,
/// as the tool it asks for and what it would run.
const TOOL = /^([A-Z][\w-]*): (.+)$/s;

/// The dialog the session shows, as its newest message: what it asks for,
/// and the question. Its options are under the thread, by the composer.
function DialogCard({ dialog, why, at, now }: { dialog: Dialog; why: string | null; at: number; now: number }) {
  const tool = why ? TOOL.exec(why) : null;
  return (
    <section className="t-dialog" aria-label="Dialog">
      <div className="t-dialog-head">
        <span className="tag tint-peach">{tool ? "Permission" : "Dialog"}</span>
        {tool && <span className="t-dialog-tool">{tool[1] === "Bash" ? "Bash command" : tool[1]}</span>}
        <Time at={at} now={now} />
      </div>
      {tool ? <div className="t-dialog-arg m">{tool[2]}</div> : why && <p className="t-dialog-why">{why}</p>}
      <p className="t-dialog-q">{dialog.question}</p>
    </section>
  );
}
