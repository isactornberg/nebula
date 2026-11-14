import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { State, Today } from "./api/contract";
import { chipOf, daysTo, TodayBand, TodayChip, todosLine } from "./Today";

// 13 November 2026, 16:00 local time.
const NOW = new Date(2026, 10, 13, 16).getTime();

const day = (over: Partial<Today> = {}): Today => ({
  at: new Date(2026, 10, 13, 3, 17).getTime(),
  todos: {
    count: 6,
    overdue: 2,
    top: [
      { title: "Renew the car insurance", due: "2026-11-01", url: "https://todo.example.com/1" },
      { title: "Book the dentist", due: "2026-11-13", url: "javascript:alert(1)" },
      { title: "Return the library books", due: "2026-11-16", url: null },
      { title: "A fourth one the file should not have sent", due: null, url: null },
    ],
    url: "https://todo.example.com/upcoming",
    problem: null,
  },
  training: {
    block: "Week 3, build",
    today: "Day 2 Lower, squat and hinge",
    url: "obsidian://open?path=%2Fx.md",
    lifts: 1,
    liftsPerWeek: 4,
    problem: null,
  },
  quiet: [{ name: "Garden shed", brain: "home", days: 47, url: null }],
  nightly: { at: new Date(2026, 10, 13, 3, 17).getTime(), ok: true, why: null },
  off: false,
  ...over,
});

const state = (today: Today | null, todayProblem: string | null = null) => ({ today, todayProblem }) as State;
const band = (s: State) => renderToStaticMarkup(<TodayBand state={s} now={NOW} />);
const chip = (s: State, fit = 0) => {
  const c = chipOf(s, NOW);
  return c ? renderToStaticMarkup(<TodayChip chip={c} fit={fit} onGo={() => {}} />) : "";
};
const text = (html: string) => html.replace(/<[^>]+>/g, "|").replace(/\|+/g, "|");

describe("the Today band", () => {
  it("is not there without a day", () => {
    expect(band(state(null))).toBe("");
    expect(chip(state(null))).toBe("");
  });

  it("counts the todos and names the top three, never more", () => {
    const t = text(band(state(day())));
    expect(t).toContain("|Todos · 6 due, 2 late|");
    expect(t).toContain("|Renew the car insurance|12d|");
    expect(t).toContain("|Book the dentist|today|");
    expect(t).toContain("|Return the library books|in 3d|");
    expect(t).not.toContain("A fourth one");
    expect(t).toContain("|+3 more ↗|");
  });

  it("links only what markdown-it keeps, a web link in a new tab and an app's in place", () => {
    const html = band(state(day()));
    expect(html).toContain('<a href="https://todo.example.com/1" target="_blank" rel="noreferrer">Renew the car insurance</a>');
    expect(html).not.toContain("javascript:");
    expect(html).toContain('<a href="obsidian://open?path=%2Fx.md">Day 2 Lower, squat and hinge ↗</a>');
  });

  it("says why what it could not read was not read", () => {
    const t = text(
      band(
        state(
          day({
            todos: { count: 0, overdue: 0, top: [], url: null, problem: "the list refused GET /tasks" },
            nightly: { at: new Date(2026, 10, 11, 3, 17).getTime(), ok: false, why: "home: push failed" },
            off: true,
          }),
        ),
      ),
    );
    expect(t).toContain("|Todos|Could not be read: the list refused GET /tasks.|");
    expect(t).toContain("|nightly failed · 2d ago|automation off|");
    expect(t).toContain("|The nightly run at 11 Nov 03:17 failed: home: push failed.|");
    expect(text(band(state(null, "The day in /x/today.json could not be read: it has no version.")))).toBe(
      "|Today|The day in /x/today.json could not be read: it has no version.|",
    );
  });

  it("says why the training could not be read in its place, and keeps that in the chip when the training gives way", () => {
    const why = "the lifting log refused GET /workout/next: 401 Invalid API key";
    const unread = state(day({ training: { block: null, today: null, url: null, lifts: 0, liftsPerWeek: 4, problem: why } }));
    const t = text(band(unread));
    expect(t).toContain(`|Training|Could not be read: ${why}.|Todos · 6 due, 2 late|`);
    expect(t).not.toContain("Nothing today");
    expect(t).not.toContain("lifts this week");
    expect(text(chip(unread))).toBe("|Today|training unread|2 late|nightly ok|");
    expect(text(chip(unread, 1))).toBe("|Today|training unread|2 late|nightly ok|");
    expect(chip(unread, 2)).toContain('<span class="error">Today</span>');
  });

  it("puts the day in a chip while the band is not on screen", () => {
    expect(text(chip(state(day())))).toBe("|Today|Day 2 Lower|2 late|nightly ok|");
    expect(text(chip(state(day({ todos: { ...day().todos, overdue: 0 }, off: true }))))).toBe(
      "|Today|Day 2 Lower|6 due|nightly ok|automation off|",
    );
    expect(text(chip(state(null, "unreadable file")))).toBe("|Today|unreadable|");
  });

  it("gives way where the top bar is short of room: the training first, then all but Today in its worst colour", () => {
    expect(text(chip(state(day()), 1))).toBe("|Today|2 late|nightly ok|");
    const alone = chip(state(day()), 2);
    expect(text(alone)).toBe("|Today|");
    expect(alone).toContain('aria-label="Today · Day 2 Lower · 2 late · nightly ok"');
    expect(alone).toContain('<span class="late">Today</span>');
    const failed = day({ nightly: { at: NOW, ok: false, why: "push failed" } });
    expect(chip(state(failed), 2)).toContain('<span class="error">Today</span>');
    const fine = day({ todos: { ...day().todos, overdue: 0 } });
    expect(chip(state(fine), 2)).toContain('<span class="ok">Today</span>');
    expect(chip(state(day()), 3), "and so while the TUI's session is cut short").toBe(alone);
  });

  it("reads due dates by the calendar and says the todos in words", () => {
    expect(daysTo("2026-11-12", NOW)).toBe(-1);
    expect(daysTo("2026-11-13T09:00:00", NOW)).toBe(0);
    expect(daysTo("soon", NOW)).toBe(null);
    expect(todosLine({ count: 0, overdue: 0, top: [], url: null, problem: null })).toBe("nothing due");
    expect(todosLine({ count: 3, overdue: 3, top: [], url: null, problem: null })).toBe("3 due, all late");
    expect(todosLine({ count: 3, overdue: 0, top: [], url: null, problem: null })).toBe("3 due");
  });
});
