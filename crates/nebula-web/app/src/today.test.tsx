import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Day, DayTile, Schedule, Schedules, State } from "./api/contract";
import { DayPills } from "./DeckPage";
import { ComingUp } from "./Idle";
import { NO_SCHEDULES } from "./schedules";
import { chipOf, daysTo, TodayChip, TodayTiles } from "./Today";

// 13 November 2026, 16:00 local time.
const NOW = new Date(2026, 10, 13, 16).getTime();
const NIGHT = new Date(2026, 10, 13, 3, 17).getTime();

const reviews: DayTile = {
  head: "Reviews",
  icon: "prOpen",
  tone: "late",
  sub: [{ text: "4 waiting" }, { text: "1 late", tone: "late" }],
  rows: [
    { text: "Retry uploads on a flaky network", due: "2026-11-01", url: "https://git.example.com/pr/1" },
    { text: "Cache the thumbnails", due: "2026-11-13", url: "javascript:alert(1)" },
    { text: "Bump the image library", due: "2026-11-16", url: "notes://open?note=Bump" },
    { text: "Tidy the README" },
  ],
  more: { text: "+1 more", url: "https://git.example.com/reviews" },
};

const release: DayTile = {
  head: "Release",
  icon: "today",
  tone: "ok",
  sub: [{ text: "2.4" }],
  main: { text: "Code freeze on Thursday", url: "https://wiki.example.com/release" },
  meter: { value: 3, of: 4, text: "3 of 4 checks green" },
};

const day = (over: Partial<Day> = {}): Day => ({
  at: NIGHT,
  tiles: [reviews, release],
  chip: [
    { text: "freeze Thu", tone: "ok" },
    { text: "1 late", tone: "late" },
    { text: "backup ok", tone: "muted", at: NIGHT },
  ],
  pills: [
    { label: "reviews", url: "https://git.example.com/reviews", words: [{ text: "4 waiting, 1 late" }] },
    { label: "release", words: [{ text: "2.4" }, { text: "3/4" }] },
    { dot: "ok", words: [{ text: "backup ok", at: NIGHT }] },
  ],
  status: [
    { text: "Backup ok", tone: "ok", at: NIGHT },
    { text: "Deploys on", tone: "ok" },
  ],
  ...over,
});

const state = (d: Day | null, dayProblem: string | null = null) => ({ day: d, dayProblem, schedules: NO_SCHEDULES as Schedules }) as State;
const band = (s: State) => renderToStaticMarkup(<TodayTiles state={s} now={NOW} />);
const coming = (s: State) => renderToStaticMarkup(<ComingUp state={s} now={NOW} onGo={() => {}} />);
const chip = (s: State) => {
  const c = chipOf(s, NOW);
  return c ? renderToStaticMarkup(<TodayChip chip={c} onGo={() => {}} />) : "";
};
const text = (html: string) => html.replace(/<[^>]+>/g, "|").replace(/\|+/g, "|");

describe("Today on Home", () => {
  it("is not there without a day, nor where a day says nothing", () => {
    for (const s of [state(null), state({ at: NIGHT, tiles: [], chip: [], pills: [], status: [] })]) {
      expect(band(s)).toBe("");
      expect(coming(s)).toBe("");
      expect(chip(s)).toBe("");
    }
  });

  it("draws each tile's head and words, and its rows with how soon each is due", () => {
    const html = band(state(day()));
    expect(html).toContain('<span class="home-sub">written 03:17</span>');
    expect(html).toContain('class="icon late" data-icon="prOpen"');
    const t = text(html);
    expect(t).toContain("|Reviews · 4 waiting, |1 late|");
    expect(html).toContain('<span class="late">1 late</span>');
    expect(t).toContain("|Retry uploads on a flaky network|12d|");
    expect(t).toContain("|Cache the thumbnails|today|");
    expect(t).toContain("|Bump the image library|in 3d|");
    expect(t).toContain("|Tidy the README|+1 more ↗|");
    expect(t).toContain("|Release · 2.4|Code freeze on Thursday ↗|3 of 4 checks green|");
    expect(html).toContain('<span style="width:75%"></span>');
  });

  it("links only what markdown-it keeps, a web link in a new tab and an app's in place", () => {
    const html = band(state(day()));
    expect(html).toContain('<a href="https://git.example.com/pr/1" target="_blank" rel="noreferrer">Retry uploads on a flaky network</a>');
    expect(html).not.toContain("javascript:");
    expect(html).toContain('<span>Cache the thumbnails</span>');
    expect(html).toContain('<a href="notes://open?note=Bump">Bump the image library</a>');
    expect(html).toContain(
      '<a href="https://wiki.example.com/release" class="tile-main" target="_blank" rel="noreferrer">Code freeze on Thursday ↗</a>',
    );
  });

  it("says a line in its tone, and a tile's problem as given in place of its body", () => {
    const quiet: DayTile = { head: "Release", main: { text: "Nothing planned", tone: "muted" }, meter: { value: 9, of: 4, text: "over" } };
    const html = band(state(day({ tiles: [quiet] })));
    expect(html).toContain('<span class="muted">Nothing planned</span>');
    expect(html).toContain('<span style="width:100%"></span>');
    expect(html).not.toContain("data-icon");

    const why = "Could not be read: the forge refused GET /pulls.";
    const unread = { ...reviews, icon: "rocket", sub: [], problem: why };
    const t = text(band(state(day({ tiles: [unread] }))));
    expect(t).toBe(`|Today|written 03:17|Reviews|${why}|+1 more ↗|`);
    expect(text(band(state(null, "The day in /x/day.json could not be read: it has no version.")))).toBe(
      "|Today|The day in /x/day.json could not be read: it has no version.|",
    );
  });

  it("says how things stand under Coming up, with the schedules that run next", () => {
    const rank: Schedule = {
      id: "01rank",
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
      when: { kind: "at", hour: 7, minute: 30, days: ["mon"] },
      nextAt: new Date(2026, 10, 14, 7, 30).getTime(),
      createdAt: 0,
      sessionId: null,
    };
    const s = { ...state(day()), schedules: { ...NO_SCHEDULES, schedules: [rank] } };
    const html = coming(s);
    expect(text(html)).toContain("|Backup ok · 03:17|Deploys on|Coming up|Morning rank|tomorrow 07:30|");
    expect(html).toContain('<span aria-hidden="true" class="sdot green"></span>Backup ok · 03:17');
    expect(html).toContain('href="/schedules/01rank"');

    const failed = day({
      status: [
        { text: "Backup failed", tone: "error", at: new Date(2026, 10, 11, 3, 17).getTime(), problem: "The backup failed: the disk is full." },
        { text: "Deploys paused", tone: "late" },
        { text: "Mirror" },
      ],
    });
    const c = coming(state(failed));
    expect(text(c)).toBe("|Backup failed · 11 Nov 03:17|Deploys paused|Mirror|The backup failed: the disk is full.|");
    expect(c).toContain('class="sdot red"></span>Backup failed');
    expect(c).toContain('class="sdot peach"></span>Deploys paused');
    expect(c).toContain('class="sdot"></span>Mirror');
  });

  it("puts the day in a chip while Home is not on screen, with how long ago once a word is a day old", () => {
    expect(text(chip(state(day())))).toBe("|Today|freeze Thu|1 late|backup ok|");
    const old = day({ chip: [{ text: "backup failed", tone: "error", at: NOW - 2 * 86_400_000 }, { text: "mirror" }] });
    expect(text(chip(state(old)))).toBe("|Today|backup failed · 2d ago|mirror|");
    expect(chip(state(old))).toContain("<span>mirror</span>");
    expect(text(chip(state(null, "unreadable file")))).toBe("|Today|unreadable|");
  });

  it("says the worst of the day in the chip's dot, and all of it on hover, where its line is cut short", () => {
    const all = chip(state(day()));
    expect(all).toContain('title="Today · freeze Thu · 1 late · backup ok"');
    expect(all).toContain('class="sdot late"');
    expect(chip(state(day({ chip: [{ text: "backup failed", tone: "error" }] })))).toContain('class="sdot error"');
    expect(chip(state(day({ chip: [{ text: "freeze Thu", tone: "ok" }, { text: "mirror" }] })))).toContain('class="sdot ok"');
  });

  it("puts the day's pills along the Deck's top, each opening its own link", () => {
    const html = renderToStaticMarkup(<DayPills pills={day().pills} now={NOW} />);
    expect(html).toContain(
      '<a href="https://git.example.com/reviews" class="deck-pill" target="_blank" rel="noreferrer"><span class="lab">reviews</span>4 waiting, 1 late</a>',
    );
    expect(html).toContain('<span class="deck-pill"><span class="lab">release</span>2.4 · 3/4</span>');
    expect(html).toContain('<span class="deck-pill deck-sys"><span aria-hidden="true" class="sdot green"></span>backup ok</span>');
  });

  it("reads due dates by the calendar", () => {
    expect(daysTo("2026-11-12", NOW)).toBe(-1);
    expect(daysTo("2026-11-13T09:00:00", NOW)).toBe(0);
    expect(daysTo("soon", NOW)).toBe(null);
  });
});
