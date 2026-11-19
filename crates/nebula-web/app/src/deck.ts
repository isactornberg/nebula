import type { SessionDetail } from "./api/contract";
import { answersShown } from "./model";

/// How the tiles are laid out on a screen `w` by `h` CSS px: 8 on a phone,
/// 12 to 16 on a tablet or a larger screen, each about half again as wide as
/// it is tall, the shape the phone's tiles have on their side. The screen's
/// size alone sets it, not the bar's, so selecting another session never
/// moves a tile. The room the day and the bar take is guessed.
export function deckGrid(w: number, h: number): { cols: number; rows: number } {
  const large = Math.min(w, h) >= 600;
  const [least, most] = large ? [12, 16] : [8, 8];
  const tilesW = w - 24;
  const tilesH = h - 160;
  let best = { cols: 4, rows: 2, off: Infinity };
  for (let cols = 2; cols <= 6; cols++) {
    for (let rows = 2; rows <= 6; rows++) {
      const n = cols * rows;
      if (n < least || n > most) continue;
      const off = Math.abs(Math.log(tilesW / cols / (tilesH / rows) / 1.5));
      if (off < best.off - 1e-9 || (Math.abs(off - best.off) < 1e-9 && n > best.cols * best.rows)) {
        best = { cols, rows, off };
      }
    }
  }
  return { cols: best.cols, rows: best.rows };
}
/// How long a press is held to read its session.
export const HOLD_MS = 500;
/// How long the tiles keep their places after a touch.
export const STILL_MS = 3000;

/// Is `pathname` the Deck's page? It is a page of its own, outside the
/// app's routes.
export function isDeck(pathname: string): boolean {
  return /^\/deck\/?$/.test(pathname);
}

/// The tiles while they hold still: each session keeps the place it had in
/// `held`, one gone since leaves its place empty, and one new since comes
/// after them all, in `live`'s order.
export function holdOrder(held: (string | null)[], live: string[]): (string | null)[] {
  const now = new Set(live);
  const kept = held.map((id) => (id !== null && now.has(id) ? id : null));
  const had = new Set(held);
  return [...kept, ...live.filter((id) => !had.has(id))];
}

/// How far a finger may stray from where it went down and still tap or
/// hold: a fingertip's roll, between Android's 8 dp and the 15 px Chrome
/// allows on a desktop.
export const SLOP_PX = 10;
/// How far a finger must go sideways for a swipe to turn the page.
export const SWIPE_PX = 40;

/// A touch on the tiles, at CSS px positions, as one of three and never two:
/// let go before `ms` within SLOP_PX of where it went down is a tap; held
/// that long within it and then let go is a hold, and `armed` says the
/// moment it is held long enough and when that ends; gone sideways past
/// SLOP_PX before that, and let go at least SWIPE_PX to one side, more
/// across than up or down, is a swipe, `1` to the next page as the finger
/// went left and `-1` to the one before. A hold acts on letting go, so what
/// it opens never comes up under the finger while it still presses, where
/// the browser's own long press would land on it. A touch that strays up or
/// down first, or once it is held, is none of them, and so is one cancelled.
export function press(
  on: { tap: () => void; hold: () => void; swipe: (by: 1 | -1) => void; armed: (armed: boolean) => void },
  ms = HOLD_MS,
) {
  let phase: "idle" | "pressed" | "armed" | "sliding" | "strayed" = "idle";
  let x0 = 0;
  let y0 = 0;
  let dx = 0;
  let dy = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const end = () => {
    clearTimeout(timer);
    if (phase === "armed") on.armed(false);
    phase = "idle";
  };
  const move = (x: number, y: number) => {
    if (phase === "idle" || phase === "strayed") return;
    dx = x - x0;
    dy = y - y0;
    if (phase === "sliding" || Math.hypot(dx, dy) <= SLOP_PX) return;
    const across = phase === "pressed" && Math.abs(dx) > Math.abs(dy);
    end();
    phase = across ? "sliding" : "strayed";
  };
  return {
    down(x: number, y: number) {
      end();
      phase = "pressed";
      x0 = x;
      y0 = y;
      dx = 0;
      dy = 0;
      timer = setTimeout(() => {
        phase = "armed";
        on.armed(true);
      }, ms);
    },
    move,
    up(x: number, y: number) {
      move(x, y);
      const was = phase;
      end();
      if (was === "pressed") on.tap();
      else if (was === "armed") on.hold();
      else if (was === "sliding" && Math.abs(dx) >= SWIPE_PX && Math.abs(dx) > Math.abs(dy)) on.swipe(dx < 0 ? 1 : -1);
    },
    cancel: end,
  };
}

/// What the bar under the tiles offers: the open dialog's options, else the
/// reply's answers, recommended first, each by the number `pickFor` takes.
export function offered(d: SessionDetail): { n: number; label: string }[] {
  if (d.dialog) return d.dialog.options.map((o) => ({ n: o.number, label: o.label }));
  return answersShown(d).map((label, i) => ({ n: i + 1, label }));
}
