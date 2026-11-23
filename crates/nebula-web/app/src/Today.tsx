import { useId, type ReactNode } from "react";
import type { DayLine, DayTile, DayTone, DayWord, State } from "./api/contract";
import { age, when } from "./model";
import { External, Icon, isIcon, md, RouteLink } from "./ui";

const DAY_MS = 86_400_000;

/// A link the day file gave, when markdown-it would keep it as one: a web
/// link opens in a new tab, any other (`obsidian:`) hands off to its app.
export function Out({ href, className, children }: { href?: string | null; className?: string; children: ReactNode }) {
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

/// A word as the page says it: with how long ago once what it says is more
/// than a day old.
function said(w: DayWord, now: number): string {
  return w.at !== undefined && now - w.at > DAY_MS ? `${w.text} · ${age(w.at, now)} ago` : w.text;
}

/// `words` joined with `sep` after `lead`, each with a tone in its colour,
/// the text between them one run, which the browser kerns as one.
export function Words({ words, sep, lead = "", now }: { words: DayWord[]; sep: string; lead?: string; now: number }) {
  const drawn: ReactNode[] = [];
  let run = lead;
  words.forEach((w, i) => {
    if (i > 0) run += sep;
    if (!w.tone) {
      run += said(w, now);
      return;
    }
    if (run) drawn.push(run);
    drawn.push(
      <span key={i} className={w.tone}>
        {said(w, now)}
      </span>,
    );
    run = "";
  });
  if (run) drawn.push(run);
  return drawn;
}

/// A dot in a tone's colour; with none, a faint one.
export function dotOf(tone: DayTone | undefined): string {
  return tone === "ok" ? "sdot green" : tone === "late" ? "sdot peach" : tone === "error" ? "sdot red" : "sdot";
}

/// The day's tiles, on Home and the Today tab. Read-only: each thing links
/// to where it is edited. How things stand is under Coming up.
export function TodayTiles({ state, now, head = true }: { state: State; now: number; head?: boolean }) {
  const id = useId();
  const { day, dayProblem } = state;
  if (!day?.tiles.length && !dayProblem) return null;
  return (
    <section className="today" aria-labelledby={id}>
      <div className={head ? "home-head" : "sr"}>
        <h2 id={id}>Today</h2>
        {day && <span className="home-sub">written {when(day.at, now)}</span>}
      </div>
      {dayProblem && <p className="error">{dayProblem}</p>}
      {day && day.tiles.length > 0 && (
        <div className="tiles">
          {day.tiles.map((tile, i) => (
            <Tile key={i} tile={tile} now={now} />
          ))}
        </div>
      )}
    </section>
  );
}

function Tile({ tile: t, now }: { tile: DayTile; now: number }) {
  return (
    <div className="day-tile">
      <h3 className="tile-head">
        {t.icon && isIcon(t.icon) && <Icon name={t.icon} size={15} className={t.tone} />}
        {t.head}
        {t.sub && t.sub.length > 0 && <Words words={t.sub} sep=", " lead=" · " now={now} />}
      </h3>
      {t.problem ? (
        <p className="error">{t.problem}</p>
      ) : (
        <>
          {t.main && <Line line={t.main} className="tile-main" />}
          {t.meter && (
            <span className="lifts">
              <span className="lifts-bar" aria-hidden="true">
                <span style={{ width: `${Math.min(100, (100 * t.meter.value) / Math.max(1, t.meter.of))}%` }} />
              </span>
              {t.meter.text}
            </span>
          )}
          {t.rows?.map((row, i) => {
            const d = row.due ? daysTo(row.due, now) : null;
            return (
              <div key={i} className="tile-row">
                <span className="clip" title={row.text}>
                  <Out href={row.url}>{row.text}</Out>
                </span>
                {d !== null && (
                  <span className={d < 0 ? "late" : "muted"}>{d < 0 ? `${-d}d` : d === 0 ? "today" : `in ${d}d`}</span>
                )}
              </div>
            );
          })}
        </>
      )}
      {t.more && <Line line={t.more} className="more" />}
    </div>
  );
}

/// A line of a tile, ending on ` ↗` when it links; one with a tone is said
/// in its colour in place of the line's own look.
function Line({ line, className }: { line: DayLine; className: string }) {
  return (
    <Out href={line.url} className={line.tone ?? className}>
      {line.text}
      {line.url && " ↗"}
    </Out>
  );
}

/// A word of the chip, in its colour.
type ChipPart = { text: string; tone?: DayTone };
export type Chip = { parts: ChipPart[]; tone: "ok" | "late" | "error" };

/// What the day's line at the list's foot says, `unreadable` first when the
/// file could not be read, with the colour of the worst of it.
export function chipOf(state: State, now: number): Chip | null {
  const { day, dayProblem } = state;
  const parts: ChipPart[] = (day?.chip ?? []).map((w) => ({ text: said(w, now), tone: w.tone }));
  if (dayProblem) parts.unshift({ text: "unreadable", tone: "error" });
  if (parts.length === 0) return null;
  const tone = parts.some((p) => p.tone === "error") ? "error" : parts.some((p) => p.tone === "late") ? "late" : "ok";
  return { parts, tone };
}

/// The day in the list's foot while Home, where its tiles are, is not on
/// screen. It goes to Home.
export function TodayChip({ chip, onGo }: { chip: Chip; onGo: () => void }) {
  const title = ["Today", ...chip.parts.map((p) => p.text)].join(" · ");
  return (
    <RouteLink to={{ kind: "idle" }} onGo={onGo} className="today-chip" title={title}>
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
