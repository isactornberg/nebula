import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, useDetail, useLiveState } from "./api/client";
import type { Project, State } from "./api/contract";
import { Detail, IssueDetail } from "./Detail";
import { Idle } from "./Idle";
import {
  cardAnswers,
  cursorFor,
  followFlash,
  itemKey,
  itemRoute,
  keyAction,
  listItems,
  moveCursor,
  parseRoute,
  pickFor,
  routePath,
  sectionOf,
  showFlash,
  type Flash,
  type FocusItem,
  type Route,
  type Section,
} from "./model";
import { onThisMachine, Phones } from "./Phones";
import { Side } from "./Side";
import { chipOf, TodayChip, useChipFit } from "./Today";
import { SkillsPage } from "./SkillsPage";
import { FlashSpot, RouteLink, type Said } from "./ui";

const FOLLOW_KEY = "nebula.followTui";
const NO_PROJECTS: Project[] = [];

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
  /// The issue whose Start row is open.
  const [asking, setAsking] = useState<string | null>(null);
  const [said, setSaid] = useState<Said | null>(null);
  const saidSeq = useRef(0);
  const now = useNow();
  const items = useMemo(() => (state ? listItems(state) : []), [state]);
  const section = sectionOf(route);
  const openId = route.kind === "session" ? route.id : null;
  const session = state?.sessions.find((s) => s.id === openId) ?? null;
  const openKey = route.kind === "issue" ? route.key : null;
  const issue = state?.board.lanes.flatMap((l) => l.issues).find((i) => i.key === openKey) ?? null;
  const openPath = route.kind === "session" || route.kind === "issue" ? routePath(route) : null;
  const refreshLanded = !!state && state.refresh.at !== null && !state.refresh.running;
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
    setAsking(null);
  }, []);

  // The path is made canonical once, on load: `/anything` is `/`.
  useEffect(() => {
    if (location.pathname !== routePath(route)) history.replaceState(null, "", routePath(route));
    const pop = () => {
      setRoute(parseRoute(location.pathname));
      setNote(null);
      setError(null);
      setAsking(null);
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

  // An issue that left the lanes is shown where it went: the session now
  // working on it, else nowhere. Not before the issues are read.
  useEffect(() => {
    if (!state || !openKey || issue || !refreshLanded) return;
    const on = state.sessions.find((s) => s.issue?.key === openKey);
    if (on) return navigate({ kind: "session", id: on.id }, true);
    navigate({ kind: "idle" }, true);
    setNote(`No issue ${openKey} in the lanes.`);
  }, [state, openKey, issue, refreshLanded, navigate]);

  // The cursor moves to the row that opens, by whatever means, once it is
  // on the list; a state push alone never moves it.
  const openListed = !!openPath && items.some((i) => routePath(itemRoute(i)) === openPath);
  useEffect(() => {
    if (openPath) setCursor((c) => cursorFor(items, c, parseRoute(openPath)));
  }, [openPath, openListed]);

  // The page reads Linear and the pull requests once it finds nothing read
  // yet, and after that only on Refresh.
  const unread = !!state && state.refresh.at === null && !state.refresh.running;
  useEffect(() => {
    if (unread) void api.refresh();
  }, [unread]);

  const tui = state?.tui ?? null;
  const tuiId = tui?.sessionId ?? null;
  // Following keeps the Attention page where the TUI is; the Skills page is
  // left alone until the user goes back.
  const tuiRoute: Route = tuiId ? { kind: "session", id: tuiId } : { kind: "idle" };
  useEffect(() => {
    if (follow && tui && section === "attention") navigate(tuiRoute, true);
  }, [follow, tui !== null, tuiId, navigate, section]);

  useEffect(() => {
    if (section !== "attention") return;
    document.title = session ? `${session.name} · nebula` : issue ? `${issue.key} · nebula` : "nebula";
  }, [session?.name, issue?.key, section]);

  // Opening a finish nobody has looked at is looking at it, as putting it
  // up in a TUI is.
  const unseen = session?.status === "finished" && session.unseen;
  useEffect(() => {
    if (session && unseen) void api.seen(session.id);
  }, [session?.id, unseen]);

  useEffect(() => {
    if (!said) return;
    const t = setTimeout(() => setSaid(null), said.error ? FLASH_ERROR_MS : FLASH_MS);
    return () => clearTimeout(t);
  }, [said]);
  const say = (at: string, flash: Flash) => setSaid({ ...flash, at, seq: ++saidSeq.current });

  const toggleFollow = () => {
    setFollow(!follow);
    if (!follow && tui && section === "skills") navigate(tuiRoute);
    const flash = !follow && tui && section === "attention" ? followFlash(route, tui) : null;
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
    navigate(itemRoute(item));
  };
  const refresh = () => {
    if (!state || state.refresh.running) return;
    void api.refresh().then((err) => err && say("refresh", { text: err, error: true }));
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
  const ask = (open: boolean) => {
    setError(null);
    setAsking(open && issue ? issue.key : null);
  };
  const start = (project: string, again: boolean) => {
    if (!issue || busy) return;
    const key = issue.key;
    const started = api.startIssue(key, { project, again }).then((r) => (r.ok ? null : r.error));
    void run(`issue:${key}`, started).then((ok) => ok && setAsking((a) => (a === key ? null : a)));
  };

  useEffect(() => {
    if (section !== "attention") return;
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
          if (asking) return ask(false);
          return openPath ? goIdle() : setCursor(null);
        case "start":
          return issue && !asking ? ask(true) : undefined;
        case "terminal":
          return showInTerminal();
        case "refresh":
          return refresh();
        case "pick":
          return pick(action.n);
      }
    };
    addEventListener("keydown", onKey);
    return () => removeEventListener("keydown", onKey);
  });

  const pickOf = items.find((i) => i.kind === "focus" && i.sessionId === openId);

  const attention = (
    <>
      <Side
        state={state}
        items={items}
        cursor={cursor}
        openPath={openPath}
        now={now}
        refreshSaid={said?.at === "refresh" ? said : null}
        onOpenRow={openRow}
        onRefresh={refresh}
      />
      {!state ? (
        <main className="pane idle">
          <p className="muted">Connecting…</p>
        </main>
      ) : issue ? (
        <IssueDetail
          key={issue.key}
          issue={issue}
          state={state}
          now={now}
          asking={asking === issue.key}
          busy={busy && actedOn === `issue:${issue.key}`}
          error={actedOn === `issue:${issue.key}` ? error : null}
          onBack={goIdle}
          onAsk={ask}
          onStart={start}
        />
      ) : openKey && !refreshLanded ? (
        <main className="pane detail">
          <p className="muted">Reading the issues…</p>
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
    </>
  );

  return (
    <div
      className="app"
      data-view={session || route.kind === "issue" || route.kind === "skill" || route.kind === "skillFixes" ? "detail" : "idle"}
    >
      <Header
        state={state}
        connected={connected}
        now={now}
        section={section}
        idle={section === "attention" && route.kind === "idle"}
        follow={follow}
        flash={said?.at === "follow" ? said : null}
        onFollow={toggleFollow}
        onGo={navigate}
      />
      <div className="body">
        {section === "skills" ? (
          <SkillsPage
            openId={route.kind === "skill" ? route.id : null}
            fixes={route.kind === "skillFixes"}
            projects={state?.projects ?? NO_PROJECTS}
            onGo={navigate}
          />
        ) : (
          attention
        )}
      </div>
    </div>
  );
}

function Header({
  state,
  connected,
  now,
  section,
  idle,
  follow,
  flash,
  onFollow,
  onGo,
}: {
  state: State | null;
  connected: boolean;
  now: number;
  section: Section;
  /// The idle view is up, with the day in its band.
  idle: boolean;
  follow: boolean;
  flash: Said | null;
  onFollow: () => void;
  onGo: (to: Route) => void;
}) {
  const tui = state?.tui;
  const shown = tui?.sessionId ? state?.sessions.find((s) => s.id === tui.sessionId) : undefined;
  const chip = state && !idle ? chipOf(state, now) : null;
  const bar = useRef<HTMLElement>(null);
  const fit = useChipFit(
    bar,
    JSON.stringify([connected, !!tui, shown?.projectName, shown?.name, chip]),
  );
  return (
    <header className="top" ref={bar} data-fit={fit}>
      <RouteLink to={{ kind: "idle" }} onGo={() => onGo({ kind: "idle" })} className="mark">
        <svg aria-hidden="true" width="16" height="16" viewBox="0 0 24 24" fill="none" strokeWidth="2">
          <circle cx="12" cy="12" r="3" />
          <circle cx="12" cy="12" r="9" strokeDasharray="3 3" />
        </svg>
        <span className="mark-name">nebula</span>
      </RouteLink>
      <nav className="sections" aria-label="Sections">
        <RouteLink
          to={{ kind: "idle" }}
          onGo={() => onGo({ kind: "idle" })}
          aria-current={section === "attention" ? "page" : undefined}
        >
          Attention
        </RouteLink>
        <RouteLink
          to={{ kind: "skills" }}
          onGo={() => onGo({ kind: "skills" })}
          aria-current={section === "skills" ? "page" : undefined}
        >
          Skills
        </RouteLink>
      </nav>
      <div className="top-right">
        {chip && <TodayChip chip={chip} fit={fit} onGo={() => onGo({ kind: "idle" })} />}
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
                  title={`${shown.projectName} › ${shown.name}`}
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
        {onThisMachine() && <Phones />}
      </div>
    </header>
  );
}
