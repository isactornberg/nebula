import type { SessionDetail, State } from "./api/contract";
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
/// How long a press is held to mark a session seen.
export const HOLD_MS = 500;
/// How long the tiles keep their places after a touch.
export const STILL_MS = 3000;

/// Is `pathname` the Deck's page? It is a page of its own, outside the
/// app's routes.
export function isDeck(pathname: string): boolean {
  return /^\/deck\/?$/.test(pathname);
}

/// The sessions the Deck has tiles for: the board's rows section by
/// section, every one that needs you first, then the sessions the board
/// leaves off, idle, stopped or folded under a captain, in the order the
/// state sends them. The orchestrator is never one.
export function deckIds(state: State): string[] {
  const known = new Set(state.sessions.map((s) => s.id));
  const rows = state.board.sections.flatMap((s) => s.rows.map((r) => r.sessionId)).filter((id) => known.has(id));
  const onBoard = new Set(rows);
  const rest = state.sessions.filter(
    (s) => !onBoard.has(s.id) && s.role !== "orchestrator" && s.id !== state.orchestratorId,
  );
  return [...rows, ...rest.map((s) => s.id)];
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

/// A press on a tile: let go before `ms` is a tap, held that long is a hold,
/// never both. A press that leaves the tile is neither.
export function press(onTap: () => void, onHold: () => void, ms = HOLD_MS) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let down = false;
  const cancel = () => {
    down = false;
    clearTimeout(timer);
  };
  return {
    down() {
      cancel();
      down = true;
      timer = setTimeout(() => {
        down = false;
        onHold();
      }, ms);
    },
    up() {
      if (!down) return;
      cancel();
      onTap();
    },
    cancel,
  };
}

/// What the bar under the tiles offers: the open dialog's options, else the
/// reply's answers, recommended first, each by the number `pickFor` takes.
export function offered(d: SessionDetail): { n: number; label: string }[] {
  if (d.dialog) return d.dialog.options.map((o) => ({ n: o.number, label: o.label }));
  return answersShown(d).map((label, i) => ({ n: i + 1, label }));
}
