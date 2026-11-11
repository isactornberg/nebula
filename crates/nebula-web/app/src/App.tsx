import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, useDetail, useLiveState } from "./api/client";
import type { State } from "./api/contract";
import { Detail } from "./Detail";
import { Idle } from "./Idle";
import {
  cardAnswers,
  cursorFor,
  followFlash,
  itemKey,
  keyAction,
  listItems,
  moveCursor,
  parseRoute,
  pickFor,
  routePath,
  showFlash,
  type Flash,
  type FocusItem,
  type Route,
} from "./model";
import { Side } from "./Side";
import { FlashSpot, RouteLink } from "./ui";

const FOLLOW_KEY = "nebula.followTui";

/// A flash and the control it is said under: `follow`, or
/// `terminal:<session id>`.
type Said = Flash & { at: string; seq: number };

/// How long a flash stays: a refusal long enough to be read.
const FLASH_MS = 2500;
const FLASH_ERROR_MS = 6000;

function readFollow(): boolean {
  try {
    return localStorage.getItem(FOLLOW_KEY) === "1";
  } catch {
    return false;
  }
}

function useNow(): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 15_000);
    return () => clearInterval(t);
  }, []);
  return now;
}

function isTextField(t: HTMLElement): boolean {
  return t.isContentEditable || t.matches("input, textarea, select");
}

export function App() {
  const { state, connected } = useLiveState();
  const [route, setRoute] = useState<Route>(() => parseRoute(location.pathname));
  const [cursor, setCursor] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [follow, setFollow] = useState(readFollow);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [actedOn, setActedOn] = useState<string | null>(null);
  const [said, setSaid] = useState<Said | null>(null);
  const saidSeq = useRef(0);
  const now = useNow();
  const items = useMemo(() => (state ? listItems(state) : []), [state]);
  const openId = route.kind === "session" ? route.id : null;
  const session = state?.sessions.find((s) => s.id === openId) ?? null;
  const detail = useDetail(
    session?.id ?? null,
    session ? `${session.status}:${session.statusChangedAt}:${session.unseen}:${session.why}` : "",
    session?.statusChangedAt ?? 0,
  );

  const navigate = useCallback((to: Route, replace = false) => {
    const path = routePath(to);
    if (location.pathname !== path) history[replace ? "replaceState" : "pushState"](null, "", path);
    setRoute(to);
    setNote(null);
    setError(null);
  }, []);

  // The path is made canonical once, on load: `/anything` is `/`.
  useEffect(() => {
    if (location.pathname !== routePath(route)) history.replaceState(null, "", routePath(route));
    const pop = () => {
      setRoute(parseRoute(location.pathname));
      setNote(null);
      setError(null);
    };
    addEventListener("popstate", pop);
    return () => removeEventListener("popstate", pop);
  }, []);

  useEffect(() => {
    if (state && openId && !session) {
      navigate({ kind: "idle" }, true);
      setNote(`No session with id ${openId}.`);
    }
  }, [state, openId, session, navigate]);

  // The cursor moves to the session that opens, by whatever means; a state
  // push alone never moves it.
  useEffect(() => {
    if (openId) setCursor((c) => cursorFor(items, c, openId));
  }, [openId, state === null]);

  const tui = state?.tui ?? null;
  const tuiId = tui?.sessionId ?? null;
  useEffect(() => {
    if (follow && tui) navigate(tuiId ? { kind: "session", id: tuiId } : { kind: "idle" }, true);
  }, [follow, tui !== null, tuiId, navigate]);

  useEffect(() => {
    document.title = session ? `${session.name} · nebula` : "nebula";
  }, [session?.name]);

  // Opening a finish nobody has looked at is looking at it, as putting it
  // up in a TUI is.
  const unread = session?.status === "finished" && session.unseen;
  useEffect(() => {
    if (session && unread) void api.seen(session.id);
  }, [session?.id, unread]);

  useEffect(() => {
    if (!said) return;
    const t = setTimeout(() => setSaid(null), said.error ? FLASH_ERROR_MS : FLASH_MS);
    return () => clearTimeout(t);
  }, [said]);
  const say = (at: string, flash: Flash) => setSaid({ ...flash, at, seq: ++saidSeq.current });

  const toggleFollow = () => {
    setFollow(!follow);
    const flash = !follow && tui ? followFlash(route, tui) : null;
    if (flash) say("follow", flash);
    else setSaid((s) => (s?.at === "follow" ? null : s));
    try {
      localStorage.setItem(FOLLOW_KEY, follow ? "0" : "1");
    } catch {
      // Not remembered across reloads, then.
    }
  };

  const openSession = (id: string) => navigate({ kind: "session", id });
  const goIdle = () => navigate({ kind: "idle" });
  const openRow = (index: number) => {
    const item = items[index];
    if (!item) return;
    setCursor(itemKey(item));
    openSession(item.sessionId);
  };
  const moveBy = (delta: 1 | -1) => {
    const at = items.findIndex((i) => itemKey(i) === cursor);
    const next = moveCursor(at < 0 ? null : at, delta, items.length);
    setCursor(next === null ? null : itemKey(items[next]!));
  };
  const run = async (id: string, request: Promise<string | null>) => {
    setBusy(true);
    setError(null);
    setActedOn(id);
    const err = await request;
    setBusy(false);
    setError(err);
    return err === null;
  };
  const canShow = !!tui;
  const showInTerminal = () => {
    if (!openId || !canShow) return;
    const at = `terminal:${openId}`;
    void api.show(openId).then((got) => say(at, showFlash(got)));
  };
  const answerCard = (item: FocusItem, n: number) => {
    const s = state?.sessions.find((x) => x.id === item.sessionId);
    const text = s && !busy ? cardAnswers(item, s)[n - 1] : undefined;
    if (text !== undefined) void run(item.sessionId, api.send(item.sessionId, text));
  };
  const pick = (n: number) => {
    if (!openId) {
      const at = items.find((i) => itemKey(i) === cursor);
      if (at?.kind === "focus") answerCard(at, n);
      return;
    }
    const p = detail && !busy ? pickFor(n, detail) : null;
    if (!p || !detail) return;
    const id = detail.session.id;
    void run(id, p.kind === "option" ? api.answer(id, p.option) : api.send(id, p.text));
  };
  const send = (text: string) => (openId ? run(openId, api.send(openId, text)) : Promise.resolve(false));

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.isComposing) return;
      const t = e.target instanceof HTMLElement ? e.target : null;
      if (t && isTextField(t)) {
        if (e.key === "Escape") t.blur();
        return;
      }
      const action = keyAction(e);
      if (!action) return;
      // Enter on a link or button is that control's own click.
      if (action.kind === "open" && t?.closest("a[href], button")) return;
      e.preventDefault();
      switch (action.kind) {
        case "move":
          return moveBy(action.delta);
        case "open": {
          const at = items.findIndex((i) => itemKey(i) === cursor);
          return openRow(at);
        }
        case "back":
          return openId ? goIdle() : setCursor(null);
        case "terminal":
          return showInTerminal();
        case "pick":
          return pick(action.n);
      }
    };
    addEventListener("keydown", onKey);
    return () => removeEventListener("keydown", onKey);
  });

  const pickOf = items.find((i) => i.kind === "focus" && i.sessionId === openId);

  return (
    <div className="app" data-view={session ? "detail" : "idle"}>
      <Header
        state={state}
        connected={connected}
        follow={follow}
        flash={said?.at === "follow" ? said : null}
        onFollow={toggleFollow}
        onGo={navigate}
      />
      <div className="body">
        <Side state={state} items={items} cursor={cursor} openId={openId} now={now} onOpenRow={openRow} />
        {!state ? (
          <main className="pane idle">
            <p className="muted">Connecting…</p>
          </main>
        ) : session ? (
          <Detail
            key={session.id}
            state={state}
            session={session}
            detail={detail}
            pick={pickOf?.kind === "focus" ? { n: pickOf.n, reason: pickOf.reason } : null}
            focus={state.focus}
            now={now}
            busy={busy}
            error={error}
            canShow={canShow}
            shown={said?.at === `terminal:${session.id}` ? said : null}
            onBack={goIdle}
            onTerminal={showInTerminal}
            onPick={pick}
            onSend={send}
            onOpen={openSession}
          />
        ) : (
          <Idle
            state={state}
            items={items}
            cursor={cursor}
            now={now}
            note={note}
            busy={busy}
            error={error}
            actedOn={actedOn}
            onOpen={openSession}
            onAnswer={answerCard}
          />
        )}
      </div>
    </div>
  );
}

function Header({
  state,
  connected,
  follow,
  flash,
  onFollow,
  onGo,
}: {
  state: State | null;
  connected: boolean;
  follow: boolean;
  flash: Said | null;
  onFollow: () => void;
  onGo: (to: Route) => void;
}) {
  const tui = state?.tui;
  const shown = tui?.sessionId ? state?.sessions.find((s) => s.id === tui.sessionId) : undefined;
  return (
    <header className="top">
      <RouteLink to={{ kind: "idle" }} onGo={() => onGo({ kind: "idle" })} className="mark">
        <svg aria-hidden="true" width="16" height="16" viewBox="0 0 24 24" fill="none" strokeWidth="2">
          <circle cx="12" cy="12" r="3" />
          <circle cx="12" cy="12" r="9" strokeDasharray="3 3" />
        </svg>
        nebula
      </RouteLink>
      <div className="top-right">
        {!connected && (
          <span className="offline" role="status">
            <span aria-hidden="true" className="dot tone-red" />
            Disconnected
          </span>
        )}
        {state?.tui && (
          <span className="tui">
            <svg
              aria-hidden="true"
              width="14"
              height="14"
              viewBox="0 0 24 24"
              fill="none"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <polyline points="4 17 10 11 4 5" />
              <line x1="12" y1="19" x2="20" y2="19" />
            </svg>
            {shown ? (
              <>
                TUI on{" "}
                <RouteLink
                  to={{ kind: "session", id: shown.id }}
                  onGo={() => onGo({ kind: "session", id: shown.id })}
                  className="tui-session"
                >
                  {shown.projectName} › {shown.name}
                </RouteLink>
              </>
            ) : (
              "TUI open"
            )}
          </span>
        )}
        {state?.tui && (
          <span className="flash-anchor">
            <button
              type="button"
              className="btn follow"
              aria-pressed={follow}
              title="Keeps this page on the session your TUI shows."
              onClick={onFollow}
            >
              {/* Both labels hold the width, so the bar does not shift. */}
              <span className="swap">
                <span className={follow ? undefined : "off"}>Following TUI</span>
                <span className={follow ? "off" : undefined}>Follow TUI</span>
              </span>
            </button>
            <FlashSpot flash={flash} />
          </span>
        )}
      </div>
    </header>
  );
}
