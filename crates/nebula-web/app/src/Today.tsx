import { useId, type ReactNode } from "react";
import type { State, Today, TodayTodos } from "./api/contract";
import { age, when } from "./model";
import { External, Icon, md, RouteLink } from "./ui";

/// `text` ending on a stop, as a sentence the page says.
const stop = (text: string) => (/[.!?]$/.test(text) ? text : `${text}.`);

/// How many todos the band names; the rest are a count.
const TOP = 3;
const DAY_MS = 86_400_000;

/// A link the day file gave, when markdown-it would keep it as one: a web
/// link opens in a new tab, any other (`obsidian:`) hands off to its app.
export function Out({ href, className, children }: { href: string | null; className?: string; children: ReactNode }) {
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
export function nightlyWord(t: Today, now: number): string | null {
  if (!t.nightly) return null;
  const word = t.nightly.ok ? "nightly ok" : "nightly failed";
  return now - t.nightly.at > DAY_MS ? `${word} · ${age(t.nightly.at, now)} ago` : word;
}

/// Today on Home: today's training and the todos due, each a tile. Read-only: each thing links to where it is edited.
/// How the nightly job and the automation stand is under Coming up.
export function TodayTiles({ state, now, head = true }: { state: State; now: number; head?: boolean }) {
  const id = useId();
  const { today: t, todayProblem } = state;
  if (!t && !todayProblem) return null;
  const shown = t ? Math.min(TOP, t.todos.top.length) : 0;
  return (
    <section className="today" aria-labelledby={id}>
      <div className={head ? "home-head" : "sr"}>
        <h2 id={id}>Today</h2>
        {t && <span className="home-sub">written {when(t.at, now)}</span>}
      </div>
      {todayProblem && <p className="error">{todayProblem}</p>}
      {t && (
        <div className="tiles">
          {t.training && (
            <div className="day-tile">
              <h3 className="tile-head">
                <Icon name="training" size={15} className="ok" />
                Training{t.training.block && ` · ${t.training.block}`}
              </h3>
              {t.training.problem ? (
                <p className="error">Could not be read: {stop(t.training.problem)}</p>
              ) : (
                <>
                  {t.training.today ? (
                    <Out href={t.training.url} className="tile-main">
                      {t.training.today}
                      {t.training.url && " ↗"}
                    </Out>
                  ) : (
                    <span className="muted">Nothing today</span>
                  )}
                  <span className="lifts">
                    <span className="lifts-bar" aria-hidden="true">
                      <span style={{ width: `${Math.min(100, (100 * t.training.lifts) / Math.max(1, t.training.liftsPerWeek))}%` }} />
                    </span>
                    {t.training.lifts} of {t.training.liftsPerWeek} lifts this week
                  </span>
                </>
              )}
            </div>
          )}
          <div className="day-tile">
            <h3 className="tile-head">
              <Icon name="todos" size={15} className="late" />
              Todos{!t.todos.problem && <> · {dueWords(t.todos)}</>}
            </h3>
            {t.todos.problem ? (
              <p className="error">Could not be read: {stop(t.todos.problem)}</p>
            ) : (
              t.todos.top.slice(0, TOP).map((todo, i) => {
                const d = todo.due ? daysTo(todo.due, now) : null;
                return (
                  <div key={i} className="tile-row">
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
            {t.todos.count > shown && (
              <Out href={t.todos.url} className="more">
                +{t.todos.count - shown} more ↗
              </Out>
            )}
          </div>
        </div>
      )}
    </section>
  );
}

/// `todosLine`, its late part in the colour of late.
function dueWords(todos: TodayTodos): ReactNode {
  const { count, overdue } = todos;
  if (count === 0 || overdue === 0) return todosLine(todos);
  return (
    <>
      {count} due, <span className="late">{overdue === count ? "all" : overdue} late</span>
    </>
  );
}

/// The nightly run and the automation, as Coming up says them: `Nightly ok
/// · 03:17` with the day before it once it is not today, and the run's
/// failure on a line of its own.
export function nightlyChip(t: Today, now: number): { text: string; ok: boolean } | null {
  if (!t.nightly) return null;
  return { text: `${t.nightly.ok ? "Nightly ok" : "Nightly failed"} · ${when(t.nightly.at, now)}`, ok: t.nightly.ok };
}

export function nightlyFailure(t: Today, now: number): string | null {
  return t.nightly && !t.nightly.ok && t.nightly.why ? `The nightly run at ${when(t.nightly.at, now)} failed: ${stop(t.nightly.why)}` : null;
}

/// A word of the chip, in its colour.
type ChipPart = { text: string; tone: "ok" | "late" | "error" | "muted" };
export type Chip = { parts: ChipPart[]; tone: "ok" | "late" | "error" };

/// What the day's line at the list's foot says: today's training up to its
/// first comma, the todos late or due, the nightly run's word, and what went
/// wrong, with the colour of the worst of it.
export function chipOf(state: State, now: number): Chip | null {
  const { today: t, todayProblem } = state;
  if (!t && !todayProblem) return null;
  const parts: ChipPart[] = [];
  if (todayProblem) parts.push({ text: "unreadable", tone: "error" });
  if (t?.training?.problem) parts.push({ text: "training unread", tone: "error" });
  else if (t?.training?.today) parts.push({ text: t.training.today.split(",")[0]!, tone: "ok" });
  if (t?.todos.problem) parts.push({ text: "todos unread", tone: "error" });
  else if (t && t.todos.overdue > 0) parts.push({ text: `${t.todos.overdue} late`, tone: "late" });
  else if (t && t.todos.count > 0) parts.push({ text: `${t.todos.count} due`, tone: "muted" });
  const nightly = t && nightlyWord(t, now);
  if (nightly) parts.push({ text: nightly, tone: t.nightly!.ok ? "muted" : "error" });
  if (t?.off) parts.push({ text: "automation off", tone: "late" });
  const tone = parts.some((p) => p.tone === "error") ? "error" : parts.some((p) => p.tone === "late") ? "late" : "ok";
  return { parts, tone };
}

/// The day in the list's foot while Home, where its tiles are, is not on
/// screen. It goes to Home.
export function TodayChip({ chip, onGo }: { chip: Chip; onGo: () => void }) {
  const said = ["Today", ...chip.parts.map((p) => p.text)].join(" · ");
  return (
    <RouteLink to={{ kind: "idle" }} onGo={onGo} className="today-chip" title={said}>
      <span aria-hidden="true" className={`sdot ${chip.tone}`} />
      <span className="today-word">Today</span>
      {chip.parts.map((p, i) => (
        <span key={i} className={p.tone}>
          {p.text}
        </span>
      ))}
    </RouteLink>
  );
}
