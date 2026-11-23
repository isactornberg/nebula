import { useEffect, useLayoutEffect, useRef, type KeyboardEvent, type ReactNode, type RefObject } from "react";
import type { Issue, Project, Session, State, TuiState } from "./api/contract";
import {
  age,
  handedLine,
  itemKey,
  itemRoute,
  look,
  orchestratorOf,
  priorityLabel,
  refreshed,
  routePath,
  rowMeta,
  type ListItem,
  type ListView,
  type Route,
} from "./model";
import { FlashSpot, Icon, RouteLink, type Said } from "./ui";

const GROUP_TITLE: Record<string, string> = {
  needsYou: "Needs you",
  done: "Done",
  running: "Running",
  captains: "Captains",
  inFlight: "In flight, no session",
  todo: "Todo",
  found: "Matches",
};

export const VIEW_TITLE: Record<ListView, string> = {
  inbox: "Inbox",
  running: "Running",
  issues: "Issues",
  projects: "Projects",
  today: "Today",
  more: "More",
};

/// Where the list was scrolled to last.
let listScroll = 0;

/// The list pane: its head, with the project it shows, the find, what it
/// lists, and at its foot what stands beside the list.
export function ListPane({
  title,
  count,
  filter,
  add,
  find,
  foot,
  children,
}: {
  title: string;
  /// How many rows it lists, said beside the title.
  count: string | null;
  filter: ReactNode;
  /// New session, at the head's end on a phone.
  add?: ReactNode;
  find: ReactNode;
  foot: ReactNode;
  children: ReactNode;
}) {
  // A page that takes the list's place, as a session's on a phone, finds it
  // scrolled where it was once it is back.
  const scroll = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    scroll.current!.scrollTop = listScroll;
  }, []);
  return (
    <section className="list" aria-label={title}>
      <div className="list-top">
        <h1>{title}</h1>
        {count && <span className="list-count">{count}</span>}
        {filter}
        {add}
      </div>
      {find}
      <div className="list-scroll" ref={scroll} onScroll={(e) => (listScroll = e.currentTarget.scrollTop)}>
        {children}
      </div>
      {foot && <div className="list-foot">{foot}</div>}
    </section>
  );
}

/// Which project the list shows: all of them, or one.
export function ProjectPick({
  projects,
  project,
  onProject,
}: {
  projects: Project[];
  project: string | null;
  onProject: (id: string | null) => void;
}) {
  return (
    <label className="project-pick">
      <Icon name="folder" size={14} />
      <select
        value={project ?? ""}
        aria-label="Project the list shows"
        onChange={(e) => onProject(e.target.value || null)}
      >
        <option value="">All projects</option>
        {projects.map((p) => (
          <option key={p.id} value={p.id}>
            {p.name}
          </option>
        ))}
      </select>
      <Icon name="chevronDown" size={12} />
    </label>
  );
}

/// Find a session: the sessions whose name, project, branch or issue key
/// hold every word typed, in the list's place while it has a word.
export function FindField({
  query,
  find,
  onQuery,
  onKey,
}: {
  query: string;
  find: RefObject<HTMLInputElement | null>;
  onQuery: (q: string) => void;
  onKey: (e: KeyboardEvent<HTMLInputElement>) => void;
}) {
  return (
    <div className="find">
      <Icon name="search" size={15} />
      <input
        ref={find}
        id="session-find"
        type="search"
        value={query}
        onChange={(e) => onQuery(e.target.value)}
        onKeyDown={onKey}
        placeholder="Find a session"
        aria-label="Find a session by its name, project, branch or issue"
        aria-keyshortcuts="/"
        enterKeyHint="go"
        autoCapitalize="none"
        autoCorrect="off"
        autoComplete="off"
        spellCheck={false}
      />
      {query ? (
        // A press leaves the focus where it was, in the field or out of
        // it; from the keyboard, the focus goes back into the field.
        <button
          type="button"
          className="find-clear"
          aria-label="Clear the find"
          onMouseDown={(e) => e.preventDefault()}
          onClick={(e) => {
            const keyed = document.activeElement === e.currentTarget;
            onQuery("");
            if (keyed) find.current?.focus();
          }}
        >
          <Icon name="close" size={14} />
        </button>
      ) : (
        <kbd aria-hidden="true">/</kbd>
      )}
    </div>
  );
}

/// How long a `leaving` row takes to fold away: a beat in place, then out.
export const ROW_LEAVE_MS = 420;
const LEAVE_DELAY_MS = 140;

/// The rows of a view, under the head of each group, or of a find's
/// matches; what it says when there are none; and, in the Inbox, how many
/// sessions the board leaves off.
export function Rows({
  state,
  view,
  items,
  cursor,
  openPath,
  now,
  query,
  onOpenRow,
}: {
  state: State;
  view: ListView;
  items: ListItem[];
  cursor: string | null;
  /// The path of the session or issue open, if any.
  openPath: string | null;
  now: number;
  query: string;
  onOpenRow: (index: number) => void;
}) {
  const nav = useRef<HTMLElement>(null);
  useEffect(() => {
    nav.current?.querySelector("[data-cursor]")?.scrollIntoView({ block: "nearest" });
  }, [cursor]);
  // A leaving row, or a group of nothing else, folds shut from its height.
  useLayoutEffect(() => {
    const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
    for (const el of nav.current?.querySelectorAll<HTMLElement>("[data-leaving]") ?? []) {
      if (el.getAnimations?.().length || !el.animate) continue;
      const shut = { height: "0px", minHeight: "0px", paddingTop: "0px", paddingBottom: "0px", opacity: 0 };
      el.animate(still ? [{ opacity: 1 }, { opacity: 0 }] : [{ height: `${el.offsetHeight}px` }, shut], {
        duration: ROW_LEAVE_MS - LEAVE_DELAY_MS,
        delay: LEAVE_DELAY_MS,
        easing: "ease-in-out",
        fill: "forwards",
      });
    }
  });

  const byId = new Map(state.sessions.map((s) => [s.id, s]));
  const groups: { group: string; entries: { item: ListItem; index: number }[] }[] = [];
  items.forEach((item, index) => {
    const group = item.kind === "issue" ? item.lane : item.group;
    const last = groups.at(-1);
    if (last?.group === group) last.entries.push({ item, index });
    else groups.push({ group, entries: [{ item, index }] });
  });
  const titleOf = (group: string) =>
    view === "projects" && !query.trim()
      ? (state.projects.find((p) => p.id === group)?.name ?? state.sessions.find((s) => s.projectId === group)?.projectName ?? group)
      : (GROUP_TITLE[group] ?? group);

  const words = query.trim().split(/\s+/);
  const finding = query.trim() !== "";
  const board = state.board;
  const rest = [board.idle && `${board.idle} idle`, board.stopped && `${board.stopped} stopped`].filter(Boolean).join(" · ");

  return (
    <nav className="rows" aria-label={finding ? "Matches" : VIEW_TITLE[view]} ref={nav}>
      {groups.map(({ group, entries }) => {
        const leaving = (item: ListItem) => item.kind === "row" && !!item.leaving;
        const gone = entries.every((e) => leaving(e.item));
        return (
          <section key={group} aria-labelledby={`group-${group}`} data-leaving={gone || undefined} inert={gone}>
            <h2 id={`group-${group}`} className="group-head">
              {titleOf(group)}
              <span className="group-n">{entries.filter((e) => !leaving(e.item)).length}</span>
            </h2>
            {entries.map(({ item, index }) => {
              const to = itemRoute(item);
              const s = item.kind === "row" ? byId.get(item.sessionId) : undefined;
              if (item.kind === "row" && !s) return null;
              const out = !gone && leaving(item);
              return (
                <RouteLink
                  key={itemKey(item)}
                  to={to}
                  onGo={() => onOpenRow(index)}
                  className={item.kind === "issue" ? "srow issue-row" : "srow"}
                  data-cursor={itemKey(item) === cursor || undefined}
                  data-leaving={out || undefined}
                  inert={out}
                  aria-current={routePath(to) === openPath ? "page" : undefined}
                >
                  {item.kind === "issue" ? (
                    <IssueRow issue={item.issue} now={now} />
                  ) : (
                    <SessionRow s={s!} workers={item.workers} now={now} project={view !== "projects" || finding} />
                  )}
                </RouteLink>
              );
            })}
          </section>
        );
      })}
      {items.length === 0 && (
        <p className="list-note">
          {finding ? (
            <>
              No session has {words.length > 1 && "every word of "}“{words.join(" ")}” in its name, project, branch or
              issue.
            </>
          ) : (
            emptyNote(state, view)
          )}
        </p>
      )}
      {view === "issues" && items.length === 0 && state.refresh.problems.length > 0 && (
        <ul className="list-note problems">
          {state.refresh.problems.map((p) => (
            <li key={p} className="error">
              {p}
            </li>
          ))}
        </ul>
      )}
      {view === "inbox" && !finding && rest && <p className="list-note">{rest}</p>}
    </nav>
  );
}

function emptyNote(state: State, view: ListView): string {
  switch (view) {
    case "running":
      return "No session is running.";
    case "issues":
      return !state.projects.some((p) => p.team)
        ? "No project reads its issues from Linear. A project's Issues row on the TUI's Project tab names its team, and LINEAR_API_KEY lets nebula read it."
        : state.refresh.at === null
          ? "Linear's issues are not read yet."
          : "No issue assigned to you waits for a session.";
    case "projects":
      return "No session yet.";
    default:
      return "Nothing on the board.";
  }
}

/// A session's row: its dot, name and age, and under them where it is and
/// what it says, with what it asks or its pull request in a pill.
export function SessionRow({
  s,
  workers,
  now,
  project = true,
  state = false,
}: {
  s: Session;
  workers: number;
  now: number;
  /// Names its project, as a row not under its project's head does.
  project?: boolean;
  /// Names its state where no pill does, as a row under no state's head does.
  state?: boolean;
}) {
  const l = look(s);
  const said = state && l.pill?.text !== l.label;
  const meta = [said && l.label, rowMeta(s, workers, project)].filter(Boolean).join(" · ");
  return (
    <>
      <span aria-hidden="true" className={l.pulse ? `sdot ${l.tone} pulse` : `sdot ${l.tone}`} />
      <span className="srow-text">
        <span className="srow-top">
          {l.pill?.text !== l.label && !said && <span className="sr">{l.label}:</span>}
          <span className="srow-title">{s.name}</span>
          <span className="srow-age">{age(s.statusChangedAt, now)}</span>
        </span>
        {(meta || l.pill) && (
          <span className="srow-sub">
            <span className="srow-meta" title={meta}>
              {meta}
            </span>
            {l.pill && <span className={`tag tint-${l.pill.tint}`}>{l.pill.text}</span>}
          </span>
        )}
      </span>
    </>
  );
}

/// An issue's row: Linear's mark for its priority, its title and when it
/// last changed, its key and state, and whom Start handed it to.
function IssueRow({ issue, now }: { issue: Issue; now: number }) {
  const started = issue.started;
  return (
    <>
      <Priority p={issue.priority} />
      <span className="srow-text">
        <span className="srow-top">
          <span className="srow-title" title={issue.title}>
            {issue.title}
          </span>
          <span className="srow-age">{age(issue.updatedAt, now)}</span>
        </span>
        <span className="srow-sub">
          <span className="srow-meta">
            <span className="m">{issue.key}</span> · {issue.state}
          </span>
          {started && (
            <span className="handed" title={handedLine(started, now)}>
              <span className="handed-to">handed to {started.projectName}</span>
              <span aria-hidden="true">·</span>
              {started.held ? <span className="held">brief held</span> : <span>{age(started.at, now)}</span>}
            </span>
          )}
        </span>
      </span>
    </>
  );
}

/// Linear's priority mark: a filled square with a `!` for urgent, as many
/// of three bars lit as it is high, and three dashes for none.
export function Priority({ p }: { p: number }) {
  const label = priorityLabel(p);
  const level = p >= 1 && p <= 4 ? p : 0;
  return (
    <svg className={`priority p${level}`} role="img" aria-label={label} width="14" height="14" viewBox="0 0 14 14">
      <title>{label}</title>
      {level === 0 ? (
        [0, 1, 2].map((i) => <rect key={i} x={1.5 + i * 4} y="6.25" width="3" height="1.5" rx="0.75" className="dim" />)
      ) : level === 1 ? (
        <>
          <rect x="1" y="1" width="12" height="12" rx="3" className="lit" />
          <rect x="6.25" y="3.5" width="1.5" height="4.5" rx="0.75" className="mark" />
          <rect x="6.25" y="9.25" width="1.5" height="1.5" rx="0.75" className="mark" />
        </>
      ) : (
        [0, 1, 2].map((i) => (
          <rect
            key={i}
            x={1.5 + i * 4}
            y={9 - i * 3}
            width="3"
            height={4 + i * 3}
            rx="1"
            className={i < 5 - level ? "lit" : "dim"}
          />
        ))
      )}
    </svg>
  );
}

/// The session the TUI shows, linked, and Follow TUI, which keeps the page
/// on it.
export function TuiLine({
  state,
  follow,
  flash,
  onFollow,
  onGo,
}: {
  state: State;
  follow: boolean;
  flash: Said | null;
  onFollow: () => void;
  onGo: (to: Route) => void;
}) {
  const tui: TuiState | null = state.tui;
  if (!tui) return null;
  const shown = tui.sessionId ? state.sessions.find((s) => s.id === tui.sessionId) : undefined;
  return (
    <div className="tui-line">
      <Icon name="terminal" size={14} />
      <span className="tui-on">
        {shown ? (
          <>
            TUI on{" "}
            <RouteLink
              to={{ kind: "session", id: shown.id }}
              onGo={() => onGo({ kind: "session", id: shown.id })}
              title={`${shown.projectName} › ${shown.name}`}
            >
              {shown.projectName} › {shown.name}
            </RouteLink>
          </>
        ) : (
          "TUI open"
        )}
      </span>
      <span className="flash-anchor">
        <button
          type="button"
          className="quiet-btn follow"
          aria-pressed={follow}
          title="Keeps this page on the session your TUI shows."
          onClick={onFollow}
        >
          {/* Both labels hold the width, so the line does not shift. */}
          <span className="swap">
            <span className={follow ? undefined : "off"}>Following TUI</span>
            <span className={follow ? "off" : undefined}>Follow TUI</span>
          </span>
        </button>
        <FlashSpot flash={flash} />
      </span>
    </div>
  );
}

/// The orchestrator's thread in one tap, a phone's top bar's: a dot while
/// its reply waits unread or it waits on you, and faint while none runs.
export function OrchestratorButton({ state, onGo }: { state: State; onGo: (to: Route) => void }) {
  const o = orchestratorOf(state);
  const l = o && look(o);
  const said = l ? `Orchestrator: ${l.label}` : "Orchestrator: not running";
  const dot = l && (l.tone === "blue" || l.tone === "peach" || l.tone === "red") ? l.tone : null;
  return (
    <RouteLink
      to={{ kind: "orchestrator" }}
      onGo={() => onGo({ kind: "orchestrator" })}
      className="round orch-open"
      aria-label={said}
      title={said}
      data-none={o ? undefined : ""}
    >
      <Icon name="sparkle" size={19} />
      {dot && <span aria-hidden="true" className={`sdot ${dot} round-dot`} />}
    </RouteLink>
  );
}

/// When Linear's issues and the pull requests were read, Refresh, and why
/// part of the last read failed.
export function RefreshLine({
  state,
  now,
  said,
  onRefresh,
}: {
  state: State;
  now: number;
  said: Said | null;
  onRefresh: () => void;
}) {
  const { refresh } = state;
  // The clock ticks in silence: a screen reader hears a refresh start and
  // land, not every minute that passes.
  const heard = refresh.running
    ? "Refreshing…"
    : refresh.at === null
      ? ""
      : refresh.problems.length > 0
        ? refresh.problems.join(" ")
        : "Refreshed.";
  return (
    <div className="refresh">
      <div className="refresh-line">
        <span className="refresh-when">{refreshed(refresh, now)}</span>
        <span className="sr" role="status">
          {heard}
        </span>
        <span className="flash-anchor">
          <button
            type="button"
            className="quiet-btn"
            onClick={onRefresh}
            aria-busy={refresh.running || undefined}
            aria-keyshortcuts="r"
            title="Reads Linear's issues assigned to you and the sessions' pull requests again."
          >
            <Icon name="refresh" size={13} />
            Refresh <kbd aria-hidden="true">r</kbd>
          </button>
          <FlashSpot flash={said} />
        </span>
      </div>
      {refresh.problems.length > 0 && (
        <ul className="refresh-problems">
          {refresh.problems.map((p) => (
            <li key={p} className="error">
              {p}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/// The orchestrator, as a link to its thread: how it stands, or that none
/// runs.
export function OrchestratorLink({
  state,
  now,
  open,
  className,
  onGo,
}: {
  state: State;
  now: number;
  /// Its thread is the page shown.
  open: boolean;
  className: string;
  onGo: (to: Route) => void;
}) {
  const o = orchestratorOf(state);
  const l = o && look(o);
  return (
    <RouteLink
      to={{ kind: "orchestrator" }}
      onGo={() => onGo({ kind: "orchestrator" })}
      className={className}
      aria-current={open ? "page" : undefined}
      aria-keyshortcuts="o"
      title="Orchestrator (O)"
    >
      <Icon name="sparkle" />
      <span className="item-name">Orchestrator</span>
      <span className="orch-state">
        {o && l ? (
          <>
            <span aria-hidden="true" className={l.pulse ? `sdot ${l.tone} pulse` : `sdot ${l.tone}`} />
            {l.label} · {age(o.statusChangedAt, now)}
          </>
        ) : (
          "Not running"
        )}
      </span>
    </RouteLink>
  );
}
