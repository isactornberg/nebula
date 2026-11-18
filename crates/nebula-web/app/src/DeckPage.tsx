import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, useDetail, useLiveState } from "./api/client";
import type { Session, State, Today } from "./api/contract";
import { deckGrid, deckIds, holdOrder, offered, press, STILL_MS } from "./deck";
import { age, pickFor, showFlash, statusLook, type Flash } from "./model";
import { nightlyWord, Out, todosLine } from "./Today";
import { Dot, useNow } from "./ui";

const FLASH_MS = 2500;
const FLASH_ERROR_MS = 6000;

/// The page for a phone on a stand beside the keyboard, `/deck`: a tile for
/// each session, tapped to put it up in the TUI and held to mark it seen,
/// the selected one's answers under them, and the day along the top.
export function Deck() {
  const { state, connected } = useLiveState();
  const now = useNow();
  useWakeLock();
  useEffect(() => {
    document.title = "Deck · nebula";
  }, []);

  const live = useMemo(() => (state ? deckIds(state) : []), [state]);
  const { order, touch } = useStill(live);
  const [page, setPage] = useState(0);
  const [picked, setPicked] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [said, setSaid] = useState<(Flash & { seq: number }) | null>(null);
  const seq = useRef(0);
  const say = (flash: Flash) => setSaid({ ...flash, seq: ++seq.current });
  useEffect(() => {
    if (!said) return;
    const t = setTimeout(() => setSaid(null), said.error ? FLASH_ERROR_MS : FLASH_MS);
    return () => clearTimeout(t);
  }, [said]);

  const byId = useMemo(() => new Map(state?.sessions.map((s) => [s.id, s])), [state]);
  const selectedId = picked !== null && live.includes(picked) ? picked : (live[0] ?? null);
  const selected = selectedId === null ? null : (byId.get(selectedId) ?? null);
  const detail = useDetail(
    selected?.id ?? null,
    selected ? `${selected.status}:${selected.statusChangedAt}:${selected.unseen}:${selected.why}` : "",
    selected?.statusChangedAt ?? 0,
  );

  const { cols, rows, wide } = useGrid();
  const per = cols * rows;
  const pages = Math.max(1, Math.ceil(order.length / per));
  const at = Math.min(page, pages - 1);
  const shown = order.slice(at * per, at * per + per);
  const dots = pages > 1 && <Dots pages={pages} at={at} onPage={setPage} />;

  const tap = (id: string) => {
    touch();
    setPicked(id);
    if (!state?.tui) return say({ text: "No TUI is open to show it in.", error: true });
    void api.show(id).then((got) => say(showFlash(got)));
  };
  const hold = (id: string) => {
    touch();
    void api.seen(id).then((err) => err && say({ text: err, error: true }));
  };
  const answer = async (n: number) => {
    const p = detail && !busy ? pickFor(n, detail) : null;
    if (!p || !detail) return;
    setBusy(true);
    const err = await api.pick(detail.session.id, p);
    setBusy(false);
    if (err) say({ text: err, error: true });
  };

  return (
    <div className="deck" onPointerDownCapture={touch}>
      {(state?.today || !connected || (wide && dots)) && (
        <div className="deck-top">
          {state?.today && <DayPills today={state.today} now={now} />}
          {!connected && (
            <span className="offline" role="status">
              <Dot tone="red" />
              Disconnected
            </span>
          )}
          {wide && dots}
        </div>
      )}
      {!state ? (
        <p className="deck-wait muted">Connecting…</p>
      ) : (
        <>
          <div
            className="deck-tiles"
            style={{ gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`, gridTemplateRows: `repeat(${rows}, minmax(0, 1fr))` }}
          >
            {Array.from({ length: per }, (_, i) => {
              const s = shown[i] ? byId.get(shown[i]!) : undefined;
              return s ? (
                <Tile
                  key={s.id}
                  s={s}
                  now={now}
                  selected={s.id === selectedId}
                  onTap={() => tap(s.id)}
                  onHold={() => hold(s.id)}
                />
              ) : (
                <div key={`empty-${i}`} className="tile empty" aria-hidden="true" />
              );
            })}
          </div>
          {!wide && dots}
          <Bar state={state} s={selected} detail={detail} busy={busy} said={said} onAnswer={answer} />
        </>
      )}
    </div>
  );
}

/// The dots that page through the tiles: at the end of the top row on a
/// screen on its side, under the tiles on one upright.
function Dots({ pages, at, onPage }: { pages: number; at: number; onPage: (page: number) => void }) {
  return (
    <nav className="deck-dots" aria-label="Pages">
      {Array.from({ length: pages }, (_, i) => (
        <button
          key={i}
          type="button"
          aria-label={`Page ${i + 1} of ${pages}`}
          aria-current={i === at ? "true" : undefined}
          onClick={() => onPage(i)}
        />
      ))}
    </nav>
  );
}

/// The tiles' grid for this screen ([`deckGrid`]), and whether it is on its
/// side, read again as it turns or is resized.
function useGrid(): { cols: number; rows: number; wide: boolean } {
  const read = () => ({ ...deckGrid(innerWidth, innerHeight), wide: innerWidth >= innerHeight });
  const [grid, setGrid] = useState(read);
  useEffect(() => {
    const resized = () => setGrid((g) => {
      const next = read();
      return next.cols === g.cols && next.rows === g.rows && next.wide === g.wide ? g : next;
    });
    addEventListener("resize", resized);
    return () => removeEventListener("resize", resized);
  }, []);
  return grid;
}

function Bar({
  state,
  s,
  detail,
  busy,
  said,
  onAnswer,
}: {
  state: State;
  s: Session | null;
  detail: ReturnType<typeof useDetail>;
  busy: boolean;
  said: (Flash & { seq: number }) | null;
  onAnswer: (n: number) => void;
}) {
  const answers = detail ? offered(detail) : [];
  return (
    <section className="deck-bar" aria-label="Selected session">
      <div className="deck-who">
        <b className="clip">{s ? `${s.projectName} › ${s.name}` : state.sessions.length ? "No session to show" : "No sessions"}</b>
        <span role="status" className={said?.error ? "clip error" : "clip"}>
          {said ? said.text : s ? (s.why ?? statusLook(s).label) : ""}
        </span>
      </div>
      {s && detail && (
        <div className="deck-answers" role="group" aria-label="Answers">
          {answers.length === 0 ? (
            <span className="muted">Nothing to answer.</span>
          ) : (
            answers.map((a, i) => (
              <button
                key={`${a.n}:${a.label}`}
                type="button"
                className={i === 0 ? "pill first" : "pill"}
                title={a.label}
                disabled={busy}
                onClick={() => onAnswer(a.n)}
              >
                {a.label}
              </button>
            ))
          )}
        </div>
      )}
    </section>
  );
}

function Tile({
  s,
  now,
  selected,
  onTap,
  onHold,
}: {
  s: Session;
  now: number;
  selected: boolean;
  onTap: () => void;
  onHold: () => void;
}) {
  const latest = useRef({ onTap, onHold });
  latest.current = { onTap, onHold };
  const p = useMemo(
    () =>
      press(
        () => latest.current.onTap(),
        () => latest.current.onHold(),
      ),
    [],
  );
  useEffect(() => p.cancel, [p]);
  const look = statusLook(s);
  const since = age(s.statusChangedAt, now);
  const rest = look.tone !== "red" && look.tone !== "yellow" && look.tone !== "blue";
  return (
    <button
      type="button"
      className={`tile tone-${look.tone}`}
      data-rest={rest || undefined}
      aria-current={selected ? "true" : undefined}
      aria-label={`${s.projectName} › ${s.name}, ${look.label}${since ? ` ${since}` : ""}. Tap to show it in the TUI, hold to mark it seen.`}
      aria-keyshortcuts="s"
      onPointerDown={(e) => e.isPrimary && e.button === 0 && p.down()}
      onPointerUp={p.up}
      onPointerLeave={p.cancel}
      onPointerCancel={p.cancel}
      onContextMenu={(e) => e.preventDefault()}
      // A click from the keyboard; a pointer's is its press.
      onClick={(e) => e.detail === 0 && onTap()}
      onKeyDown={(e) => e.key === "s" && onHold()}
    >
      <span className="tile-project clip">{s.projectName}</span>
      <span className="tile-name">{s.name}</span>
      <span className="tile-foot">
        <Dot tone={look.tone} />
        <b>{look.label}</b>
        {since}
      </span>
      {s.status === "running" && <span className="tile-run" />}
    </button>
  );
}

/// The day along the top: the todos due, today's training and the quietest
/// project, each opening its own link, and how the nightly job stands, with
/// `automation off` while the kill switch is on, read-only.
function DayPills({ today: t, now }: { today: Today; now: number }) {
  const training = t.training;
  const quiet = t.quiet[0];
  const nightly = nightlyWord(t, now);
  const bad = t.off || (t.nightly !== null && !t.nightly.ok);
  return (
    <>
      <Out href={t.todos.url} className="deck-pill">
        <span className="lab">todos</span>
        {t.todos.problem ? "unread" : todosLine(t.todos)}
      </Out>
      {training && (training.problem || training.today) && (
        <Out href={training.url} className="deck-pill">
          <span className="lab">training</span>
          {training.problem
            ? "unread"
            : `${training.today!.split(",")[0]} · ${training.lifts}/${training.liftsPerWeek}`}
        </Out>
      )}
      {quiet && (
        <Out href={quiet.url} className="deck-pill">
          <span className="lab">quiet</span>
          {quiet.name} · {quiet.days}d
        </Out>
      )}
      <span className="deck-pill deck-sys">
        <Dot tone={bad ? "red" : "green"} />
        {[nightly ?? "no nightly yet", t.off && "automation off"].filter(Boolean).join(" · ")}
      </span>
    </>
  );
}

/// The tiles in the order they hold still in: as `live` has them, until a
/// touch, then where they were for STILL_MS after the last one.
function useStill(live: string[]): { order: (string | null)[]; touch: () => void } {
  const [held, setHeld] = useState<(string | null)[] | null>(null);
  const shown = useRef<(string | null)[]>(live);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const touch = useCallback(() => {
    setHeld((h) => h ?? shown.current);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setHeld(null), STILL_MS);
  }, []);
  const order = held ? holdOrder(held, live) : live;
  shown.current = order;
  return { order, touch };
}

/// Keeps the screen on while the page is visible, asking again each time it
/// comes back, as iOS lets the lock go when it is hidden. A refusal is
/// ignored: the screen then sleeps as it would.
function useWakeLock() {
  useEffect(() => {
    let lock: WakeLockSentinel | null = null;
    let gone = false;
    const ask = () => {
      if (document.visibilityState !== "visible" || !("wakeLock" in navigator)) return;
      navigator.wakeLock.request("screen").then(
        (l) => (gone ? void l.release() : (lock = l)),
        () => {},
      );
    };
    ask();
    document.addEventListener("visibilitychange", ask);
    return () => {
      gone = true;
      document.removeEventListener("visibilitychange", ask);
      void lock?.release();
    };
  }, []);
}
