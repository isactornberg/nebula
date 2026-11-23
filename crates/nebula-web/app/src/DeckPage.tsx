import { useCallback, useEffect, useMemo, useRef, useState, type HTMLAttributes } from "react";
import { api, useDetail, useLiveState } from "./api/client";
import type { DayPill, Session, State } from "./api/contract";
import { ComposeForm, useCompose } from "./Compose";
import { deckGrid, holdOrder, offered, press, readerEsc, STILL_MS } from "./deck";
import {
  actingOn,
  age,
  answersShown,
  canSend,
  ended,
  look,
  NOT_ACTING,
  pickFor,
  replyWho,
  sessionOrder,
  showFlash,
  shownName,
  TONE_TINT,
  type Acting,
  type Flash,
} from "./model";
import { Thread, threadItems } from "./Thread";
import { dotOf, Out, Words } from "./Today";
import { Icon, isTextField, useNow } from "./ui";

const FLASH_MS = 2500;
const FLASH_ERROR_MS = 6000;

/// The page for a phone on a stand beside the keyboard, `/deck`: a tile for
/// each session, tapped to put it up in the TUI and held to read it and
/// answer it here, the selected one's answers under them, and the day along
/// the top.
export function Deck() {
  const { state, connected } = useLiveState();
  const now = useNow();
  useWakeLock();
  useEffect(() => {
    document.title = "Deck · nebula";
  }, []);

  const live = useMemo(() => (state ? sessionOrder(state) : []), [state]);
  const { order, touch } = useStill(live);
  const [page, setPage] = useState(0);
  /// The last turn of the page, which way it went, and how many there have
  /// been, so each comes in afresh.
  const [turn, setTurn] = useState<{ n: number; by: 1 | -1 } | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  /// The session the reader is open on.
  const [reading, setReading] = useState<string | null>(null);
  const [acting, setActing] = useState<Acting>(NOT_ACTING);
  const actSeq = useRef(0);
  /// What the page says for a moment, of the session `on`.
  const [said, setSaid] = useState<(Flash & { seq: number; on: string }) | null>(null);
  const seq = useRef(0);
  const say = (on: string, flash: Flash) => setSaid({ ...flash, on, seq: ++seq.current });
  const saidOf = (s: { id: string } | null) => (s && said?.on === s.id ? said : null);
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
  const turnTo = (to: number, by: 1 | -1) => {
    setPage(to);
    setTurn((t) => ({ n: (t?.n ?? 0) + 1, by }));
  };
  const dots = pages > 1 && (
    <Dots
      pages={pages}
      at={at}
      onPage={(to) => to !== at && turnTo(to, to > at ? 1 : -1)}
      onStep={(by) => turnTo((at + by + pages) % pages, by)}
    />
  );

  const tap = (id: string) => {
    touch();
    setPicked(id);
    if (!state?.tui) return say(id, { text: "No TUI is open to show it in.", error: true });
    void api.show(id).then((got) => say(id, showFlash(got)));
  };
  const read = (id: string) => {
    touch();
    setPicked(id);
    setReading(id);
    // The tiles come back as they were when the reader closes, not turning.
    setTurn(null);
  };
  // A swipe stops at the first page and the last, and the page it turns to
  // holds still as a touch has it.
  const swipe = (by: 1 | -1) => {
    if (at + by < 0 || at + by >= pages) return;
    touch();
    turnTo(at + by, by);
  };
  const { armed, handlers } = useTouches({ tap, hold: read, swipe });
  const reader = reading !== null && live.includes(reading) ? (byId.get(reading) ?? null) : null;
  // Opening a finish nobody has looked at is looking at it, as it is on the
  // rest of the page.
  const unseen = reader?.status === "finished" && reader.unseen;
  useEffect(() => {
    if (reader && unseen) void api.seen(reader.id);
  }, [reader?.id, unseen]);
  /// Asks `request` of session `on`; only the last ask holds its answers,
  /// and what went wrong is said on that session alone.
  const run = async (on: string, request: Promise<string | null>) => {
    const n = ++actSeq.current;
    setActing({ on, seq: n, busy: true, error: null });
    const err = await request;
    setActing((a) => ended(a, n, err));
    if (err) say(on, { text: err, error: true });
    return err === null;
  };
  const busyOn = (s: { id: string } | null) => !!s && actingOn(acting, s.id).busy;
  const send = (text: string, images: string[]) => (reader ? run(reader.id, api.send(reader.id, text, images)) : Promise.resolve(false));
  const answer = (n: number) => {
    const p = detail && !busyOn(detail.session) ? pickFor(n, detail) : null;
    if (p && detail) void run(detail.session.id, api.pick(detail.session.id, p));
  };

  return (
    <div className="deck" onPointerDownCapture={touch}>
      <div className="deck-top">
          <a href="/" className="back deck-back">
            <Icon name="chevronLeft" size={18} />
            Inbox
          </a>
          {state?.day && <DayPills pills={state.day.pills} now={now} />}
          {!connected && (
            <span className="offline" role="status">
              <span aria-hidden="true" className="sdot red" />
              Disconnected
            </span>
          )}
          {wide && !reader && dots}
        </div>
      {!state ? (
        <p className="deck-wait muted">Connecting…</p>
      ) : reader ? (
        <Reader
          s={reader}
          detail={detail?.session.id === reader.id ? detail : null}
          now={now}
          busy={busyOn(reader)}
          said={saidOf(reader)}
          canShow={!!state.tui}
          onShow={() => tap(reader.id)}
          onAnswer={answer}
          onSend={send}
          onClose={() => setReading(null)}
        />
      ) : (
        <>
          <div
            key={turn?.n}
            className="deck-tiles"
            data-turn={turn ? (turn.by > 0 ? "next" : "prev") : undefined}
            style={{ gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`, gridTemplateRows: `repeat(${rows}, minmax(0, 1fr))` }}
            {...handlers}
          >
            {Array.from({ length: per }, (_, i) => {
              const s = shown[i] ? byId.get(shown[i]!) : undefined;
              return s ? (
                <Tile
                  key={s.id}
                  s={s}
                  now={now}
                  selected={s.id === selectedId}
                  armed={s.id === armed}
                  onTap={() => tap(s.id)}
                  onHold={() => read(s.id)}
                />
              ) : (
                <div key={`empty-${i}`} className="tile empty" aria-hidden="true" />
              );
            })}
          </div>
          {!wide && dots}
          <Bar
            state={state}
            s={selected}
            detail={detail}
            busy={busyOn(selected)}
            said={saidOf(selected)}
            onAnswer={answer}
            onRead={() => selected && read(selected.id)}
          />
        </>
      )}
    </div>
  );
}

/// More pages than this are a count with arrows, not a dot each, so the
/// pages never crowd out the day along the top.
const MOST_DOTS = 4;

/// What pages through the tiles: at the end of the top row on a screen on
/// its side, under the tiles on one upright.
function Dots({
  pages,
  at,
  onPage,
  onStep,
}: {
  pages: number;
  at: number;
  onPage: (page: number) => void;
  onStep: (by: 1 | -1) => void;
}) {
  if (pages > MOST_DOTS) {
    const go = onStep;
    return (
      <nav className="deck-pager" aria-label="Pages">
        <button type="button" aria-label="Previous page" onClick={() => go(-1)}>
          ‹
        </button>
        <span aria-live="polite">
          {at + 1}/{pages}
        </span>
        <button type="button" aria-label="Next page" onClick={() => go(1)}>
          ›
        </button>
      </nav>
    );
  }
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
  onRead,
}: {
  state: State;
  s: Session | null;
  detail: ReturnType<typeof useDetail>;
  busy: boolean;
  said: (Flash & { seq: number }) | null;
  onAnswer: (n: number) => void;
  onRead: () => void;
}) {
  const answers = detail ? offered(detail) : [];
  const line = (
    <span role="status" className={said?.error ? "clip error" : "clip"}>
      {said ? said.text : s ? (s.why ?? look(s).label) : ""}
    </span>
  );
  return (
    <section className="deck-bar" aria-label="Selected session">
      {s ? (
        <button type="button" className="deck-who" onClick={onRead} title="Read its reply and answer it here">
          <b className="clip">
            {s.projectName} › {s.name}
            <ReadMark />
          </b>
          {line}
        </button>
      ) : (
        <div className="deck-who">
          <b className="clip">{state.sessions.length ? "No session to show" : "No sessions"}</b>
          {line}
        </div>
      )}
      {s && detail && (
        <div className="deck-answers" role="group" aria-label="Answers">
          {answers.length === 0 ? (
            <span className="muted">Nothing to answer.</span>
          ) : (
            answers.map((a, i) => (
              <button
                key={`${a.n}:${a.label}`}
                type="button"
                className={i === 0 ? "answer first" : "answer"}
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

/// The mark that says the bar opens the reader: a page turned up.
function ReadMark() {
  return (
    <svg className="read-mark" aria-hidden="true" width="12" height="12" viewBox="0 0 24 24" fill="none" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="7 14 12 9 17 14" />
    </svg>
  );
}

/// The selected session up close, over the tiles: its last reply and the
/// dialog it shows, as its page's thread draws them, its dialog's options
/// or its reply's answers, and a follow-up of your own in its page's
/// composer. Close goes back to the tiles, as Esc does once the focus is
/// out of a field with text in it ([`readerEsc`]).
function Reader({
  s,
  detail,
  now,
  busy,
  said,
  canShow,
  onShow,
  onAnswer,
  onSend,
  onClose,
}: {
  s: Session;
  detail: ReturnType<typeof useDetail>;
  now: number;
  busy: boolean;
  said: (Flash & { seq: number }) | null;
  canShow: boolean;
  onShow: () => void;
  onAnswer: (n: number) => void;
  onSend: (text: string, images: string[]) => Promise<boolean>;
  onClose: () => void;
}) {
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return;
      const field = e.target instanceof HTMLElement && isTextField(e.target) ? (e.target as HTMLInputElement) : null;
      const does = readerEsc(e, field);
      if (does === "leave") field?.blur();
      else if (does === "close") onClose();
    };
    addEventListener("keydown", key);
    return () => removeEventListener("keydown", key);
  }, [onClose]);
  const l = look(s);
  const answers = detail ? offered(detail) : [];
  // The reader's head says what the session is, so the thread starts at
  // its reply.
  const items = useMemo(() => threadItems(s, detail, null, []).filter((i) => i.kind !== "start"), [s, detail]);
  return (
    <section className="deck-reader" aria-label={`${s.projectName} › ${s.name}`}>
      <div className="deck-reader-top">
        <div className="deck-who">
          <b className="clip">
            {s.projectName} › {s.name}
          </b>
          <span role="status" className={said?.error ? "clip error" : "clip"}>
            {said ? (
              said.text
            ) : (
              <>
                <span aria-hidden="true" className={l.pulse ? `sdot ${l.tone} pulse` : `sdot ${l.tone}`} /> {l.label}
              </>
            )}
          </span>
        </div>
        {canShow && (
          <button type="button" className="btn" onClick={onShow}>
            <Icon name="monitor" size={15} />
            Show in TUI
          </button>
        )}
        <button type="button" className="btn sq" aria-label="Close" onClick={onClose}>
          <Icon name="close" size={17} />
        </button>
      </div>
      <div className="deck-reader-body">
        <div className="deck-reader-reply">
          {items.length > 0 ? (
            <Thread items={items} s={s} now={now} dropNext={!!detail && answersShown(detail).length > 0} onOpen={() => {}} />
          ) : (
            <p className="t-empty">{detail ? "No reply yet." : "Reading the reply…"}</p>
          )}
        </div>
        <div className="deck-reader-act">
          {answers.length > 0 && (
            <div className="deck-answers" role="group" aria-label={detail?.dialog ? "Answer the dialog" : "Answers"}>
              {answers.map((a, i) => (
                <button
                  key={`${a.n}:${a.label}`}
                  type="button"
                  className={i === 0 ? "answer first" : "answer"}
                  disabled={busy}
                  onClick={() => onAnswer(a.n)}
                >
                  {a.label}
                </button>
              ))}
            </div>
          )}
          <Composer session={s} dialogOpen={!!detail?.dialog} answered={answers.length > 0} busy={busy} onSend={onSend} />
        </div>
      </div>
    </section>
  );
}

/// The tiles' touches, each a tap, a hold or a swipe ([`press`]): the
/// handlers for the grid, so a swipe may start on a tile, an empty place or
/// between them, and the tile held long enough that letting go reads it.
function useTouches(on: { tap: (id: string) => void; hold: (id: string) => void; swipe: (by: 1 | -1) => void }): {
  armed: string | null;
  handlers: HTMLAttributes<HTMLDivElement>;
} {
  const latest = useRef(on);
  latest.current = on;
  /// The tile the touch went down on.
  const at = useRef<string | null>(null);
  const [armed, setArmed] = useState<string | null>(null);
  const p = useMemo(
    () =>
      press({
        tap: () => at.current && latest.current.tap(at.current),
        hold: () => at.current && latest.current.hold(at.current),
        swipe: (by) => latest.current.swipe(by),
        armed: (on) => setArmed(on ? at.current : null),
      }),
    [],
  );
  useEffect(() => p.cancel, [p]);
  return {
    armed,
    handlers: {
      onPointerDown: (e) => {
        if (!e.isPrimary || e.button !== 0) return;
        at.current = e.target instanceof Element ? (e.target.closest<HTMLElement>("[data-id]")?.dataset.id ?? null) : null;
        p.down(e.clientX, e.clientY);
      },
      onPointerMove: (e) => e.isPrimary && p.move(e.clientX, e.clientY),
      onPointerUp: (e) => e.isPrimary && p.up(e.clientX, e.clientY),
      onPointerCancel: p.cancel,
      // A mouse that leaves the tiles; a finger's touch keeps to them until it lifts.
      onPointerLeave: p.cancel,
      onContextMenu: (e) => e.preventDefault(),
    },
  };
}

function Tile({
  s,
  now,
  selected,
  armed,
  onTap,
  onHold,
}: {
  s: Session;
  now: number;
  selected: boolean;
  /// Held long enough: letting go now opens the reader.
  armed: boolean;
  onTap: () => void;
  onHold: () => void;
}) {
  const l = look(s);
  const since = age(s.statusChangedAt, now);
  const rest = l.tone === "faint" || l.tone === "hollow";
  return (
    <button
      type="button"
      className={`tile tone-${l.tone}`}
      data-rest={rest || undefined}
      data-armed={armed || undefined}
      aria-current={selected ? "true" : undefined}
      aria-label={`${s.projectName} › ${s.name}, ${l.label}${since ? ` ${since}` : ""}. Tap to show it in the TUI, hold to read it.`}
      aria-keyshortcuts="s"
      data-id={s.id}
      // A click from the keyboard; a pointer's is its touch on the tiles.
      onClick={(e) => e.detail === 0 && onTap()}
      onKeyDown={(e) => e.key === "s" && onHold()}
    >
      <span className="tile-project clip">{s.projectName}</span>
      <span className="tile-name">{s.name}</span>
      <span className="tile-foot">
        <span className={`state-pill ${TONE_TINT[l.tone]}`}>
          <span aria-hidden="true" className={`sdot ${l.tone}`} />
          {l.label}
          {since && ` · ${since}`}
        </span>
      </span>
      {s.status === "running" && <span className="tile-run" />}
    </button>
  );
}

/// The day's pills along the top, each opening its own link, read-only;
/// one with a dot says how things stand.
export function DayPills({ pills, now }: { pills: DayPill[]; now: number }) {
  return pills.map((p, i) => (
    <Out key={i} href={p.url} className={p.dot ? "deck-pill deck-sys" : "deck-pill"}>
      {p.dot && <span aria-hidden="true" className={dotOf(p.dot)} />}
      {p.label && <span className="lab">{p.label}</span>}
      <Words words={p.words} sep=" · " now={now} />
    </Out>
  ));
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

/// The reader's follow-up, in the composer of the session's page: images
/// pasted or picked, dictation, and Send.
function Composer({
  session,
  dialogOpen,
  answered,
  busy,
  onSend,
}: {
  session: Session;
  dialogOpen: boolean;
  answered: boolean;
  busy: boolean;
  onSend: (text: string, images: string[]) => Promise<boolean>;
}) {
  const stopped = session.status !== "running" && !canSend(session);
  const blocked = dialogOpen
    ? "A dialog is open: answer it first."
    : session.status === "running"
      ? "Running: send once the turn is done."
      : stopped
        ? "The session has stopped and takes no follow-up."
        : null;
  const c = useCompose(onSend, blocked, busy);
  const { who, to } = replyWho(session);
  return (
    <ComposeForm
      c={c}
      id="reply-text"
      phone
      stopped={stopped}
      words={{
        label: answered ? `Or answer ${shownName(session)} in your own words` : `Reply to ${shownName(session)}`,
        placeholder: answered ? "Your own answer…" : `Reply to ${who}…`,
        to: `Replying to ${to}`,
        send: "Send",
        dictate: "Dictate a reply",
      }}
    />
  );
}
