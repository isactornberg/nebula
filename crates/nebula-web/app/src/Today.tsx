import { useEffect, useId, useLayoutEffect, useState, type ReactNode, type RefObject } from "react";
import type { State, Today, TodayTodos } from "./api/contract";
import { age, when } from "./model";
import { External, md, RouteLink } from "./ui";

/// `text` ending on a stop, as a sentence the page says.
const stop = (text: string) => (/[.!?]$/.test(text) ? text : `${text}.`);

/// How many todos the band names; the rest are a count.
const TOP = 3;
const DAY_MS = 86_400_000;

/// A link the day file gave, when markdown-it would keep it as one: a web
/// link opens in a new tab, any other (`obsidian:`) hands off to its app.
function Out({ href, className, children }: { href: string | null; className?: string; children: ReactNode }) {
  if (!href || !md.validateLink(href)) return <span className={className}>{children}</span>;
  return /^https?:/i.test(href) ? (
    <External href={href} className={className}>
      {children}
    </External>
  ) : (
    <a href={href} className={className}>
      {children}
    </a>
  );
}

/// Calendar days from `now` to `due` (`2026-11-10`), negative once late.
export function daysTo(due: string, now: number): number | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(due);
  if (!m) return null;
  const at = new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3])).getTime();
  const today = new Date(now);
  today.setHours(0, 0, 0, 0);
  return Math.round((at - today.getTime()) / DAY_MS);
}

export function todosLine({ count, overdue }: TodayTodos): string {
  if (count === 0) return "nothing due";
  if (overdue === 0) return `${count} due`;
  return `${count} due, ${overdue === count ? "all" : overdue} late`;
}

/// The nightly run in a word, with how long ago once it is a day old.
function nightlyWord(t: Today, now: number): string | null {
  if (!t.nightly) return null;
  const word = t.nightly.ok ? "nightly ok" : "nightly failed";
  return now - t.nightly.at > DAY_MS ? `${word} · ${age(t.nightly.at, now)} ago` : word;
}

/// The bottom band of the idle view: today's training, the todos due, the
/// projects gone quiet, and how the nightly job and automation stand.
/// Read-only: each thing links to where it is edited.
export function TodayBand({ state, now }: { state: State; now: number }) {
  const id = useId();
  const { today: t, todayProblem } = state;
  if (!t && !todayProblem) return null;
  const nightly = t && nightlyWord(t, now);
  return (
    <section className="today" aria-labelledby={id}>
      <div className="today-head">
        <h2 id={id} className="caps">
          Today
        </h2>
        {t && <span className="quiet">written {when(t.at, now)}</span>}
      </div>
      {todayProblem && <p className="error">{todayProblem}</p>}
      {t && (
        <>
          <div className="today-cols">
            {t.training && (
              <div className="today-col">
                <h3 className="ok">
                  Training{t.training.block && ` · ${t.training.block}`}
                </h3>
                {t.training.problem ? (
                  <p className="error">Could not be read: {stop(t.training.problem)}</p>
                ) : (
                  <>
                    {t.training.today ? (
                      <Out href={t.training.url}>
                        {t.training.today}
                        {t.training.url && " ↗"}
                      </Out>
                    ) : (
                      <span className="muted">Nothing today</span>
                    )}
                    <span className="muted">
                      {t.training.lifts} of {t.training.liftsPerWeek} lifts this week
                    </span>
                  </>
                )}
              </div>
            )}
            <div className="today-col">
              <h3 className="late">Todos{!t.todos.problem && ` · ${todosLine(t.todos)}`}</h3>
              {t.todos.problem ? (
                <p className="error">Could not be read: {stop(t.todos.problem)}</p>
              ) : (
                t.todos.top.slice(0, TOP).map((todo, i) => {
                  const d = todo.due ? daysTo(todo.due, now) : null;
                  return (
                    <div key={i} className="today-row">
                      <span className="clip" title={todo.title}>
                        <Out href={todo.url}>{todo.title}</Out>
                      </span>
                      {d !== null && (
                        <span className={d < 0 ? "late" : "muted"}>
                          {d < 0 ? `${-d}d` : d === 0 ? "today" : `in ${d}d`}
                        </span>
                      )}
                    </div>
                  );
                })
              )}
              {t.todos.count > Math.min(TOP, t.todos.top.length) && (
                <Out href={t.todos.url} className="more">+{t.todos.count - Math.min(TOP, t.todos.top.length)} more ↗</Out>
              )}
            </div>
            <div className="today-col">
              <h3 className="muted">Quiet for 30 days or more</h3>
              {t.quiet.length === 0 ? (
                <span className="muted">Nothing has gone quiet.</span>
              ) : (
                t.quiet.map((q) => (
                  <div key={`${q.brain}/${q.name}`} className="today-row">
                    <span className="clip" title={`${q.name}, in ${q.brain}`}>
                      <Out href={q.url}>{q.name}</Out>
                    </span>
                    <span className="muted">{q.days}d</span>
                  </div>
                ))
              )}
            </div>
          </div>
          <div className="today-chips">
            {nightly && <span className={t.nightly!.ok ? "chip" : "chip error"}>{nightly}</span>}
            <span className={t.off ? "chip late" : "chip ok"}>{t.off ? "automation off" : "automation on"}</span>
          </div>
          {t.nightly && !t.nightly.ok && t.nightly.why && (
            <p className="error">
              The nightly run at {when(t.nightly.at, now)} failed: {stop(t.nightly.why)}
            </p>
          )}
        </>
      )}
    </section>
  );
}

/// A word of the chip, in its colour; the training's is the first to go.
type ChipPart = { text: string; tone: "ok" | "late" | "error" | "muted"; training?: true };
export type Chip = { parts: ChipPart[]; tone: "ok" | "late" | "error" };

/// What the chip in the top bar says of the day: today's training up to its
/// first comma, the todos late or due, the nightly run's word, and what went
/// wrong, with the colour of the worst of it. A training that could not be
/// read is not the training that gives way for room.
export function chipOf(state: State, now: number): Chip | null {
  const { today: t, todayProblem } = state;
  if (!t && !todayProblem) return null;
  const parts: ChipPart[] = [];
  if (todayProblem) parts.push({ text: "unreadable", tone: "error" });
  if (t?.training?.problem) parts.push({ text: "training unread", tone: "error" });
  else if (t?.training?.today) parts.push({ text: t.training.today.split(",")[0]!, tone: "ok", training: true });
  if (t?.todos.problem) parts.push({ text: "todos unread", tone: "error" });
  else if (t && t.todos.overdue > 0) parts.push({ text: `${t.todos.overdue} late`, tone: "late" });
  else if (t && t.todos.count > 0) parts.push({ text: `${t.todos.count} due`, tone: "muted" });
  const nightly = t && nightlyWord(t, now);
  if (nightly) parts.push({ text: nightly, tone: t.nightly!.ok ? "muted" : "error" });
  if (t?.off) parts.push({ text: "automation off", tone: "late" });
  const tone = parts.some((p) => p.tone === "error") ? "error" : parts.some((p) => p.tone === "late") ? "late" : "ok";
  return { parts, tone };
}

const PHONE = "(max-width: 767px)";

/// Whether anything at the right of the top bar `bar` has gone onto a line
/// under its mark.
function wraps(bar: HTMLElement): boolean {
  const mark = bar.querySelector(".mark")?.getBoundingClientRect();
  return !!mark && [...bar.querySelectorAll(".top-right > *")].some((e) => e.getBoundingClientRect().top >= mark.bottom);
}

/// How far the top bar `bar` gives way to keep to one line: 0 says it all,
/// 1 leaves the training out of the chip, 2 has the chip say `Today` alone
/// in the colour of the worst of it, and 3 cuts the session the TUI shows
/// short (the bar's `data-fit`). Measured again before the page is drawn,
/// after each change of `key`, what the bar holds, and of the bar's width.
/// A phone's bar is left to wrap, as its own layout has it.
export function useChipFit(bar: RefObject<HTMLElement | null>, key: string): number {
  const [fit, setFit] = useState(0);
  const [width, setWidth] = useState(0);
  useEffect(() => {
    const el = bar.current;
    if (!el) return;
    const sized = new ResizeObserver(([e]) => setWidth(Math.round(e!.contentRect.width)));
    sized.observe(el);
    return () => sized.disconnect();
  }, [bar]);
  useLayoutEffect(() => setFit(0), [key, width]);
  useLayoutEffect(() => {
    if (fit < 3 && bar.current && !matchMedia(PHONE).matches && wraps(bar.current)) setFit(fit + 1);
  });
  return fit;
}

/// The day in the top bar while the band is not on screen, as far as `fit`
/// lets it say it ([`useChipFit`]). It goes to the idle view, where the band
/// is.
export function TodayChip({ chip, fit, onGo }: { chip: Chip; fit: number; onGo: () => void }) {
  const said = ["Today", ...chip.parts.map((p) => p.text)].join(" · ");
  const parts = fit === 0 ? chip.parts : chip.parts.filter((p) => !p.training);
  return (
    <RouteLink to={{ kind: "idle" }} onGo={onGo} className="today-chip" title={said} aria-label={fit >= 2 ? said : undefined}>
      <span className={fit >= 2 ? chip.tone : undefined}>Today</span>
      {fit < 2 && parts.map((p, i) => <span key={i} className={p.tone}>{p.text}</span>)}
    </RouteLink>
  );
}
