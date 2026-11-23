import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent as FieldKey, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { api, useDetail, useLiveState, useTurns } from "./api/client";
import type { Project, Session, SessionStarted } from "./api/contract";
import { ArchivedSpot, Detail, IssueDetail, NoOrchestrator, type Layout } from "./Detail";
import { ComingUp, Home, RankAgain, Shipped, Spotlight, type HomeActs, type SayOf } from "./Idle";
import {
  actingOn,
  cardAnswers,
  cardsShown,
  composing,
  cursorFor,
  doneIndex,
  ended,
  findItems,
  focusPicks,
  followFlash,
  itemKey,
  itemRoute,
  keepDone,
  keyAction,
  moveCursor,
  NOT_ACTING,
  needsYouCount,
  orchestratorOf,
  parseRoute,
  pickFor,
  pickWhat,
  ranking,
  setAsideNote,
  routePath,
  sectionOf,
  showFlash,
  shownName,
  viewItems,
  when,
  type Acting,
  type Archived,
  type Flash,
  type FocusItem,
  type Held,
  type ListView,
  type Route,
} from "./model";
import { IS_MAC } from "./Compose";
import { afterStart, NewSessionPage, StartedAway } from "./NewSession";
import { Palette } from "./PaletteDialog";
import { pageKey, type ActionId, type PaletteCtx, type PaletteItem } from "./palette";
import { onThisMachine, PhonesDialog } from "./Phones";
import { More, PhoneTop, Rail, Tabs } from "./Shell";
import {
  FindField,
  ListPane,
  OrchestratorButton,
  OrchestratorLink,
  ProjectPick,
  RefreshLine,
  ROW_LEAVE_MS,
  Rows,
  TuiLine,
  VIEW_TITLE,
} from "./Side";
import { chipOf, TodayChip, TodayTiles } from "./Today";
import { SchedulesPage, type ScheduleView } from "./SchedulesPage";
import { SkillsPage } from "./SkillsPage";
import { toggleTheme, useTheme } from "./theme";
import { Icon, isTextField, overlayOpen, RouteLink, useMedia, useNow, type Said } from "./ui";

const FOLLOW_KEY = "nebula.followTui";
const TERMINAL_KEY = "nebula.terminal";
const NO_PROJECTS: Project[] = [];

/// How long a flash stays: a refusal long enough to be read.
const FLASH_MS = 2500;
const FLASH_ERROR_MS = 6000;
/// How long an Archive's Undo is offered.
const UNDO_MS = 8000;

/// How long a session New started may be missing from the state before
/// the page says there is none.
const NEW_WAIT_MS = 10_000;

function readFlag(key: string): boolean {
  try {
    return localStorage.getItem(key) === "1";
  } catch {
    return false;
  }
}

function keepFlag(key: string, on: boolean) {
  try {
    localStorage.setItem(key, on ? "1" : "0");
  } catch {
    // Not remembered across reloads, then.
  }
}

/// Narrower than this, Home is not beside the list: the list is the idle
/// view, as on a phone, and the day a view of it.
const SINGLE = "(max-width: 931px)";
const PHONE = "(max-width: 640px)";

export function App() {
  const { state, connected } = useLiveState();
  const [route, setRoute] = useState<Route>(() => parseRoute(location.pathname));
  const [cursor, setCursor] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [follow, setFollow] = useState(() => readFlag(FOLLOW_KEY));
  /// The live terminal beside a session's thread, or in its place.
  const [terminal, setTerminal] = useState(() => readFlag(TERMINAL_KEY));
  const [acting, setActing] = useState<Acting>(NOT_ACTING);
  const actSeq = useRef(0);
  /// The issue whose Start row is open.
  const [asking, setAsking] = useState<string | null>(null);
  const [said, setSaid] = useState<Said | null>(null);
  const saidSeq = useRef(0);
  /// What the list shows, and of which project.
  const [view, setView] = useState<ListView>("inbox");
  const [project, setProject] = useState<string | null>(null);
  /// What the find field holds: while it has a word, the list is the
  /// sessions it matches.
  const [query, setQuery] = useState("");
  const find = useRef<HTMLInputElement>(null);
  const [palette, setPalette] = useState(false);
  /// The session New started, until the state has it.
  const [pendingNew, setPendingNew] = useState<string | null>(null);
  /// The session New started once the page had gone elsewhere.
  const [away, setAway] = useState<SessionStarted | null>(null);
  const [phones, setPhones] = useState(false);
  /// The last Archive asked for, until its word at the page's foot goes.
  const [archived, setArchived] = useState<Archived | null>(null);
  const archiveSeq = useRef(0);
  /// The open session's Done row, kept in its place once opening it read
  /// it, and the one open last, folding away.
  const [held, setHeld] = useState<Held | null>(null);
  const [left, setLeft] = useState<Held | null>(null);
  const theme = useTheme();
  const now = useNow();
  const single = useMedia(SINGLE);
  const phone = useMedia(PHONE);
  const layout: Layout = phone ? "phone" : single ? "single" : "wide";
  // A wide screen has the day on Home and the rest in the rail.
  const shownView: ListView = (!single && view === "today") || (!phone && view === "more") ? "inbox" : view;
  const board = useMemo(() => (state ? viewItems(state, shownView, project) : []), [state, shownView, project]);
  const rows = useMemo(
    () => (state && shownView === "inbox" ? keepDone(board, state, held, left) : board),
    [state, shownView, board, held, left],
  );
  const finding = query.trim() !== "";
  const items = useMemo(() => (state && finding ? findItems(state, query) : rows), [state, finding, query, rows]);
  const section = sectionOf(route);
  const orchestrator = state ? orchestratorOf(state) : null;
  const openId = route.kind === "session" ? route.id : route.kind === "orchestrator" ? (orchestrator?.id ?? null) : null;
  const session = state?.sessions.find((s) => s.id === openId) ?? null;
  const openKey = route.kind === "issue" ? route.key : null;
  const issue = state?.board.lanes.flatMap((l) => l.issues).find((i) => i.key === openKey) ?? null;
  const openPath = route.kind === "session" || route.kind === "orchestrator" || route.kind === "issue" ? routePath(route) : null;
  const refreshLanded = !!state && state.refresh.at !== null && !state.refresh.running;
  const version = session ? `${session.status}:${session.statusChangedAt}:${session.unseen}:${session.why}` : "";
  const detail = useDetail(session?.id ?? null, version, session?.statusChangedAt ?? 0);
  const turns = useTurns(session?.id ?? null, version, session?.statusChangedAt ?? 0);
  const picks = useMemo(() => (state ? focusPicks(state) : []), [state]);

  const navigate = useCallback((to: Route, replace = false) => {
    const path = routePath(to);
    if (location.pathname !== path) history[replace ? "replaceState" : "pushState"](null, "", path);
    setRoute(to);
    setNote(null);
    setAway(null);
    setActing(clearError);
    setAsking(null);
  }, []);

  // The path is made canonical once, on load: `/anything` is `/`.
  useEffect(() => {
    if (location.pathname !== routePath(route)) history.replaceState(null, "", routePath(route));
    const pop = () => {
      setRoute(parseRoute(location.pathname));
      setNote(null);
      setAway(null);
      setActing(clearError);
      setAsking(null);
    };
    addEventListener("popstate", pop);
    return () => removeEventListener("popstate", pop);
  }, []);

  useEffect(() => {
    if (state && route.kind === "session" && !session && route.id !== pendingNew) {
      navigate({ kind: "idle" }, true);
      setNote(`No session with id ${route.id}.`);
    }
  }, [state, route, session, navigate, pendingNew]);

  // The orchestrator's session is its thread, at a path of its own that
  // outlives the session.
  useEffect(() => {
    if (route.kind === "session" && route.id === orchestrator?.id) navigate({ kind: "orchestrator" }, true);
  }, [route, orchestrator?.id, navigate]);

  const newSeen = !!pendingNew && !!state?.sessions.some((s) => s.id === pendingNew);
  useEffect(() => {
    if (!pendingNew) return;
    if (newSeen) return setPendingNew(null);
    const t = setTimeout(() => setPendingNew(null), NEW_WAIT_MS);
    return () => clearTimeout(t);
  }, [pendingNew, newSeen]);

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
  // on the list; a state push alone never moves it, nor does typing a find.
  const openListed = !!openPath && rows.some((i) => routePath(itemRoute(i)) === openPath);
  useEffect(() => {
    if (openPath) setCursor((c) => cursorFor(items, c, parseRoute(openPath)));
  }, [openPath, openListed]);

  // The page reads Linear and the pull requests once it finds nothing read
  // yet, and after that only on Refresh.
  const unread = !!state && state.refresh.at === null && !state.refresh.running;
  useEffect(() => {
    if (unread) void api.refresh();
  }, [unread]);

  // Opening the page, or coming back to it, asks for a rank: the server
  // asks the orchestrator only when the picks are due.
  const up = !!state;
  useEffect(() => {
    if (!up) return;
    const ask = () => document.visibilityState === "visible" && void api.rank({ auto: true });
    ask();
    document.addEventListener("visibilitychange", ask);
    return () => document.removeEventListener("visibilitychange", ask);
  }, [up]);

  const tui = state?.tui ?? null;
  const tuiId = tui?.sessionId ?? null;
  // Following keeps the sessions' page where the TUI is; the Skills and
  // Schedules pages are left alone until the user goes back.
  const tuiRoute: Route = tuiId ? { kind: "session", id: tuiId } : { kind: "idle" };
  useEffect(() => {
    if (follow && tui && section === "attention") navigate(tuiRoute, true);
  }, [follow, tui !== null, tuiId, navigate, section]);

  const tabName = session && shownName(session);
  useEffect(() => {
    if (section !== "attention") return;
    document.title =
      route.kind === "orchestrator"
        ? "Orchestrator · nebula"
        : session
          ? `${tabName} · nebula`
          : issue
            ? `${issue.key} · nebula`
            : route.kind === "new"
              ? "New session · nebula"
              : "nebula";
  }, [tabName, issue?.key, section, route.kind]);

  // Opening a finish nobody has looked at is looking at it, as putting it
  // up in a TUI is.
  const unseen = session?.status === "finished" && session.unseen;
  useEffect(() => {
    if (session && unseen) void api.seen(session.id);
  }, [session?.id, unseen]);

  // A read finish leaves Done, so the open one keeps its row there, as the
  // TUI's board keeps the session on screen, until another opens or the
  // list does, when it folds away.
  const openDone = shownView === "inbox" && session ? doneIndex(board, session.id) : -1;
  useEffect(() => {
    if (held && held.id !== openId) {
      setLeft(held);
      setHeld(null);
    }
    if (session && openDone >= 0) setHeld({ id: session.id, at: openDone, changed: session.statusChangedAt });
  }, [openId, openDone, session?.statusChangedAt]);
  useEffect(() => {
    if (!left) return;
    const t = setTimeout(() => setLeft(null), ROW_LEAVE_MS);
    return () => clearTimeout(t);
  }, [left]);

  useEffect(() => {
    if (!said) return;
    const t = setTimeout(() => setSaid(null), said.error ? FLASH_ERROR_MS : FLASH_MS);
    return () => clearTimeout(t);
  }, [said]);
  const say = (at: string, flash: Flash) => setSaid({ ...flash, at, seq: ++saidSeq.current });

  useEffect(() => {
    if (!archived || archived.phase === "archiving" || archived.phase === "undoing") return;
    const ms = archived.phase === "archived" ? UNDO_MS : archived.phase === "failed" ? FLASH_ERROR_MS : FLASH_MS;
    const t = setTimeout(() => setArchived((a) => (a === archived ? null : a)), ms);
    return () => clearTimeout(t);
  }, [archived]);
  /// Moves an Archive on to `phase` once its request answers, unless a
  /// later one took its place.
  const settle = (seq: number, phase: Archived["phase"], err: string | null) =>
    setArchived((a) => (a?.seq !== seq ? a : err ? { ...a, phase: "failed", error: err } : { ...a, phase }));
  /// Archive, confirmed on the session's page: back to the list, its URL
  /// gone with it, where the page's foot says so and offers Undo.
  const archive = (s: Session) => {
    const seq = ++archiveSeq.current;
    navigate({ kind: "idle" }, true);
    setArchived({ id: s.id, name: shownName(s), seq, phase: "archiving", error: null });
    void api.archive(s.id).then((err) => settle(seq, "archived", err));
  };
  const undoArchive = () => {
    if (archived?.phase !== "archived") return;
    const { seq, id } = archived;
    setArchived({ ...archived, phase: "undoing" });
    void api.unarchive(id).then((err) => settle(seq, "back", err));
  };

  const toggleFollow = () => {
    setFollow(!follow);
    if (!follow && tui && section !== "attention") navigate(tuiRoute);
    const flash = !follow && tui && section === "attention" ? followFlash(openId ? { kind: "session", id: openId } : route, tui) : null;
    if (flash) say("follow", flash);
    else setSaid((s) => (s?.at === "follow" ? null : s));
    keepFlag(FOLLOW_KEY, !follow);
  };
  const toggleTerminal = () => {
    setTerminal(!terminal);
    keepFlag(TERMINAL_KEY, !terminal);
  };

  const openSession = (id: string) => navigate({ kind: "session", id });
  const goIdle = () => navigate({ kind: "idle" });
  /// A view of the list goes Home with the list on it.
  const goView = (v: ListView) => {
    setView(v);
    setCursor(null);
    if (route.kind !== "idle") navigate({ kind: "idle" });
  };
  const openRow = (index: number) => {
    const item = items[index];
    if (!item) return;
    setCursor(itemKey(item));
    navigate(itemRoute(item));
  };
  /// A find puts the cursor on its first match, and clearing it puts the
  /// cursor back on the open row, if the list has one.
  const onQuery = (q: string) => {
    setQuery(q);
    const found = state && q.trim() ? findItems(state, q) : null;
    if (found) setCursor(found[0] ? itemKey(found[0]) : null);
    else setCursor(openPath ? cursorFor(rows, null, parseRoute(openPath)) : null);
  };
  /// In the find field the arrow keys move, Enter opens the cursor's match,
  /// else the first, and leaves the field, and Esc clears it, else leaves it.
  const findKey = (e: FieldKey<HTMLInputElement>) => {
    if (composing(e.nativeEvent)) return;
    switch (e.key) {
      case "ArrowDown":
      case "ArrowUp":
        e.preventDefault();
        return moveBy(e.key === "ArrowDown" ? 1 : -1);
      case "Enter": {
        e.preventDefault();
        const at = items.findIndex((i) => itemKey(i) === cursor);
        const open = at >= 0 ? at : finding && items.length > 0 ? 0 : -1;
        if (open < 0) return;
        e.currentTarget.blur();
        return openRow(open);
      }
      case "Escape":
        e.preventDefault();
        return query ? onQuery("") : e.currentTarget.blur();
    }
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
  /// Asks `request` of `on`, a session or `issue:<key>`. Only the last ask
  /// is shown, and only on its own page.
  const run = async (on: string, request: Promise<string | null>) => {
    const seq = ++actSeq.current;
    setActing({ on, seq, busy: true, error: null });
    const err = await request;
    setActing((a) => ended(a, seq, err));
    return err === null;
  };
  const busyOn = (on: string) => actingOn(acting, on).busy;
  const canShow = !!tui;
  const showInTerminal = () => {
    if (!openId || !canShow) return;
    const at = `terminal:${openId}`;
    void api.show(openId).then((got) => say(at, showFlash(got)));
  };
  const answerCard = (item: FocusItem, n: number) => {
    const id = item.sessionId;
    const s = id === null ? undefined : state?.sessions.find((x) => x.id === id);
    const text = s && !busyOn(s.id) ? cardAnswers(item, s)[n - 1] : undefined;
    if (s && text !== undefined) void run(s.id, api.send(s.id, text));
  };
  const rank = () => {
    if (!state || ranking(state)) return;
    void api.rank({ auto: false }).then((err) => err && say("rank", { text: err, error: true }));
  };
  const sayOf: SayOf = async (item, verdict, reason) => {
    const what = state ? pickWhat(item, state) : item.key;
    const err = await api.feedback({ key: item.key, verdict, reason });
    if (!err) setNote(setAsideNote(what, verdict));
    return err;
  };
  const pick = (n: number) => {
    if (cardsShown(route, single, shownView, finding)) {
      // `1`-`9` answer the card of the row the cursor is on.
      const at = items.find((i) => itemKey(i) === cursor);
      const card = at?.kind === "row" ? picks.find((p) => p.sessionId === at.sessionId) : undefined;
      if (card) answerCard(card, n);
      return;
    }
    if (!openId || !detail || busyOn(detail.session.id)) return;
    const p = pickFor(n, detail);
    if (!p) return;
    const id = detail.session.id;
    void run(id, api.pick(id, p));
  };
  const send = (text: string, images: string[]) =>
    openId ? run(openId, api.send(openId, text, images)) : Promise.resolve(false);
  const ask = (open: boolean) => {
    setActing(clearError);
    setAsking(open && issue ? issue.key : null);
  };
  const start = (project: string, again: boolean) => {
    if (!issue || busyOn(`issue:${issue.key}`)) return;
    const key = issue.key;
    const started = api.startIssue(key, { project, again }).then((r) => (r.ok ? null : r.error));
    void run(`issue:${key}`, started).then((ok) => ok && setAsking((a) => (a === key ? null : a)));
  };

  /// New started a session: its thread opens in New's place, else the page
  /// gone to since says it started.
  const started = (got: SessionStarted) => {
    setPendingNew(got.id);
    const next = afterStart(location.pathname, got);
    if (next.kind === "away") return setAway(got);
    navigate(next.to, true);
    setNote(next.note);
  };

  const act = (a: ActionId) => {
    switch (a) {
      case "reply":
        return document.getElementById("reply-text")?.focus();
      case "terminal":
        return toggleTerminal();
      case "show":
        return showInTerminal();
      case "seen":
        return session ? void api.seen(session.id) : undefined;
      case "new":
        return navigate({ kind: "new" });
      case "orchestrator":
        return navigate({ kind: "orchestrator" });
      case "rank":
        return rank();
      case "refresh":
        return refresh();
      case "follow":
        return toggleFollow();
      case "theme":
        return toggleTheme();
      case "schedules":
        return navigate({ kind: "schedules" });
      case "skills":
        return navigate({ kind: "skills" });
      case "deck":
        return location.assign("/deck");
      case "phones":
        return setPhones(true);
    }
  };
  /// The palette closes, giving the focus back, before what it picked runs.
  const fromPalette = (item: PaletteItem) => {
    flushSync(() => setPalette(false));
    switch (item.kind) {
      case "session":
        return openSession(item.session.id);
      case "issue":
        return navigate({ kind: "issue", key: item.issue.key });
      case "project":
        setQuery("");
        setProject(item.project.id);
        return goView("projects");
      case "action":
        return act(item.action);
    }
  };
  const paletteCtx: PaletteCtx = { session, follow: tui ? follow : null, theme, terminal, phones: onThisMachine() };

  // ⌘K on a Mac, Ctrl+K elsewhere, opens the palette and closes it again,
  // on every page and in a text field too; `n` opens New and `o` the
  // orchestrator's thread outside a text field, a menu and a dialog.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || composing(e)) return;
      const k = pageKey(e, IS_MAC);
      if (!k || document.querySelector("dialog[open]:not(.pal)")) return;
      const t = e.target instanceof HTMLElement ? e.target : null;
      if (k === "palette") {
        e.preventDefault();
        return setPalette(!palette);
      }
      if (palette || route.kind === k || overlayOpen() || (t && isTextField(t))) return;
      e.preventDefault();
      navigate({ kind: k });
    };
    addEventListener("keydown", onKey);
    return () => removeEventListener("keydown", onKey);
  });

  useEffect(() => {
    if (section !== "attention" || palette) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || composing(e) || overlayOpen()) return;
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
          if (openPath || route.kind === "new") return goIdle();
          return query ? onQuery("") : setCursor(null);
        case "find":
          return find.current?.focus();
        case "start":
          return issue && !asking ? ask(true) : undefined;
        case "terminal":
          return session ? toggleTerminal() : undefined;
        case "refresh":
          return refresh();
        case "pick":
          return route.kind === "new" ? undefined : pick(action.n);
      }
    };
    addEventListener("keydown", onKey);
    return () => removeEventListener("keydown", onKey);
  });

  const pickOf = picks.find((i) => (i.sessionId === null ? i.key === openKey : i.sessionId === openId));
  const byOrchestrator = state?.focus.source === "orchestrator";
  /// New's session, open before the state has it.
  const opening = route.kind === "session" && !session && openId === pendingNew;
  const open =
    !!session ||
    opening ||
    route.kind === "orchestrator" ||
    route.kind === "new" ||
    route.kind === "issue" ||
    route.kind === "skill" ||
    route.kind === "skillFixes" ||
    route.kind === "schedule" ||
    route.kind === "scheduleNew";
  const acts: HomeActs = {
    onOpen: openSession,
    onOpenIssue: (key) => navigate({ kind: "issue", key }),
    onStart: (key) => {
      navigate({ kind: "issue", key });
      setAsking(key);
    },
    onAnswer: answerCard,
    onOption: (id, option) => {
      if (!busyOn(id)) void run(id, api.answer(id, option));
    },
    onSay: sayOf,
    onRank: rank,
    onGo: navigate,
  };
  const go = {
    section,
    view: shownView,
    needsYou: state ? needsYouCount(state) : 0,
    onView: goView,
    onGo: navigate,
    making: route.kind === "new",
    schedules: !!state?.schedules.enabled,
    onPalette: () => setPalette(true),
    onPhones: () => setPhones(true),
  };
  const chip = state ? chipOf(state, now) : null;
  const rankSaid = said?.at === "rank" ? said : null;
  const tuiLine = state && (
    <TuiLine state={state} follow={follow} flash={said?.at === "follow" ? said : null} onFollow={toggleFollow} onGo={navigate} />
  );
  /// Where Back goes: the list's view, by name.
  const back = finding ? "Find" : VIEW_TITLE[shownView];
  const awayNote: ReactNode = away && <StartedAway got={away} onOpen={() => openSession(away.id)} />;
  const offline = !connected && (
    <p className="offline" role="status">
      <span aria-hidden="true" className="sdot red" />
      Disconnected
    </p>
  );
  /// What a session's or an issue's page says across its top.
  const banner = ((phone && offline) || awayNote) && (
    <>
      {phone && offline}
      {awayNote && (
        <p className="note sess-note" role="status">
          {awayNote}
        </p>
      )}
    </>
  );

  const main = !state ? (
    <main className="home">
      <p className="muted">Connecting…</p>
    </main>
  ) : route.kind === "new" ? (
    <NewSessionPage
      projects={state.projects}
      filter={project}
      layout={layout}
      back={back}
      onBack={goIdle}
      onStarted={started}
    />
  ) : opening ? (
    <main className="home">
      <p className="muted">Opening the session…</p>
    </main>
  ) : route.kind === "orchestrator" && !session ? (
    <NoOrchestrator layout={layout} back={back} banner={banner} onBack={goIdle} />
  ) : issue ? (
    <IssueDetail
      key={issue.key}
      issue={issue}
      state={state}
      now={now}
      asking={asking === issue.key}
      {...actingOn(acting, `issue:${issue.key}`)}
      layout={layout}
      back={back}
      onBack={goIdle}
      onAsk={ask}
      onStart={start}
      pick={pickOf ?? null}
      note={note}
      onSay={byOrchestrator ? sayOf : null}
      banner={banner}
    />
  ) : openKey && !refreshLanded ? (
    <main className="home">
      <p className="muted">Reading the issues…</p>
    </main>
  ) : session ? (
    <Detail
      key={session.id}
      state={state}
      session={session}
      detail={detail}
      turns={turns}
      pick={pickOf ?? null}
      note={note}
      onSay={byOrchestrator ? sayOf : null}
      focus={state.focus}
      now={now}
      {...actingOn(acting, session.id)}
      layout={layout}
      back={back}
      terminal={terminal}
      canShow={canShow}
      shown={said?.at === `terminal:${session.id}` ? said : null}
      follow={tui ? follow : null}
      banner={banner}
      onBack={goIdle}
      onTerminal={toggleTerminal}
      onShow={showInTerminal}
      onFollow={toggleFollow}
      onSeen={() => void api.seen(session.id)}
      onPick={pick}
      onSend={send}
      onOpen={openSession}
      onArchive={() => archive(session)}
    />
  ) : (
    <Home
      state={state}
      now={now}
      note={awayNote || note}
      cursor={cursor}
      acting={acting}
      rankSaid={rankSaid}
      acts={acts}
    />
  );

  const rowsView = shownView !== "today" && shownView !== "more";
  const list = (
    <ListPane
      title={finding ? "Find" : VIEW_TITLE[shownView]}
      count={
        state && rowsView
          ? countOf(items.length, finding ? "match" : shownView === "issues" ? "issue" : "session")
          : state?.day && shownView === "today"
            ? `written ${when(state.day.at, now)}`
            : null
      }
      filter={
        state && rowsView && !finding && state.projects.length > 1 ? (
          <ProjectPick projects={state.projects} project={project} onProject={setProject} />
        ) : null
      }
      add={
        phone &&
        (shownView === "inbox" || shownView === "running" || shownView === "projects") && (
          <RouteLink to={{ kind: "new" }} onGo={() => navigate({ kind: "new" })} className="list-new" aria-label="New session">
            <Icon name="plus" size={20} />
          </RouteLink>
        )
      }
      find={rowsView && <FindField query={query} find={find} onQuery={onQuery} onKey={findKey} />}
      foot={
        state &&
        !phone && (
          <>
            {tuiLine}
            {chip && route.kind !== "idle" && <TodayChip chip={chip} onGo={goIdle} />}
            <RefreshLine state={state} now={now} said={said?.at === "refresh" ? said : null} onRefresh={refresh} />
            <OrchestratorLink state={state} now={now} open={route.kind === "orchestrator"} className="orch-link" onGo={navigate} />
          </>
        )
      }
    >
      {!state ? (
        <p className="list-note">Connecting…</p>
      ) : shownView === "today" ? (
        <div className="day-view">
          <TodayTiles state={state} now={now} head={false} />
          <Shipped state={state} now={now} onOpen={openSession} />
          <ComingUp state={state} now={now} onGo={navigate} />
        </div>
      ) : shownView === "more" ? (
        <More go={go} first={<OrchestratorLink state={state} now={now} open={false} className="more-item" onGo={navigate} />}>
          <RefreshLine state={state} now={now} said={said?.at === "refresh" ? said : null} onRefresh={refresh} />
        </More>
      ) : (
        <>
          {single && shownView === "inbox" && !finding && (
            <>
              {(awayNote || note) && (
                <p className="note" role="status">
                  {awayNote || note}
                </p>
              )}
              <Spotlight
                state={state}
                now={now}
                cursor={cursor}
                acting={acting}
                acts={acts}
                rank={<RankAgain state={state} said={rankSaid} onRank={rank} />}
                left={false}
              />
            </>
          )}
          <Rows
            state={state}
            view={shownView}
            items={items}
            cursor={cursor}
            openPath={openPath}
            now={now}
            query={query}
            onOpenRow={openRow}
          />
        </>
      )}
    </ListPane>
  );

  // On a wide screen the live terminal takes the list's room.
  const wideTerminal = section === "attention" && !!session && terminal && layout === "wide";
  const body =
    section === "schedules" ? (
      <SchedulesPage state={state} view={scheduleView(route)} now={now} onGo={navigate} />
    ) : section === "skills" ? (
      <SkillsPage
        openId={route.kind === "skill" ? route.id : null}
        fixes={route.kind === "skillFixes"}
        projects={state?.projects ?? NO_PROJECTS}
        onGo={navigate}
      />
    ) : wideTerminal ? (
      main
    ) : !single ? (
      <>
        {list}
        {main}
      </>
    ) : open ? (
      main
    ) : (
      list
    );
  const panes = section === "attention" && (single || wideTerminal) ? 1 : 2;

  return (
    <div className="app" data-view={open ? "detail" : "idle"}>
      {phone ? (
        !open && (
          <PhoneTop
            orchestrator={state && <OrchestratorButton state={state} onGo={navigate} />}
            onGo={navigate}
            onSearch={() => setPalette(true)}
            onPhones={() => setPhones(true)}
          >
            {offline}
            {tuiLine}
          </PhoneTop>
        )
      ) : (
        <Rail go={go} single={single} connected={connected} />
      )}
      <div className="body" data-panes={panes}>
        {body}
      </div>
      {/* Only over a list, never over a page's own foot. */}
      <ArchivedSpot archived={open ? null : archived} onUndo={undoArchive} />
      {phone && !open && <Tabs go={go} todayTone={chip?.tone ?? null} />}
      {palette && state && (
        <Palette state={state} ctx={paletteCtx} now={now} phone={phone} onPick={fromPalette} onClose={() => setPalette(false)} />
      )}
      {onThisMachine() && <PhonesDialog open={phones} onClose={() => setPhones(false)} />}
    </div>
  );
}

const clearError = (a: Acting): Acting => (a.error === null ? a : { ...a, error: null });

function countOf(n: number, what: "session" | "issue" | "match"): string {
  if (what === "match") return n === 1 ? "1 match" : `${n} matches`;
  return `${n} ${what}${n === 1 ? "" : "s"}`;
}

function scheduleView(route: Route): ScheduleView {
  if (route.kind === "scheduleNew") return { kind: "new" };
  if (route.kind === "schedule") return { kind: "one", id: route.id };
  return { kind: "list" };
}
