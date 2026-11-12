import { useEffect, useRef, useState, type FormEvent } from "react";
import { api } from "./api/client";
import type { Issue, IssueLaneName, SectionName, Session, State } from "./api/contract";
import {
  age,
  clock,
  itemKey,
  itemRoute,
  priorityLabel,
  refreshed,
  routePath,
  sessionIssue,
  statusLook,
  type ListItem,
} from "./model";
import { Dot, FlashSpot, RouteLink, type Said } from "./ui";

const SECTION_TITLE: Record<SectionName | IssueLaneName, string> = {
  needsYou: "Needs you",
  done: "Done",
  running: "Running",
  captains: "Captains",
  inFlight: "In flight, no session",
  todo: "Todo",
};

type Entry = { item: ListItem; index: number };

export function Side({
  state,
  items,
  cursor,
  openPath,
  now,
  refreshSaid,
  onOpenRow,
  onRefresh,
}: {
  state: State | null;
  items: ListItem[];
  cursor: string | null;
  /// The path of the session or issue open, if any.
  openPath: string | null;
  now: number;
  /// What Refresh said, for a moment.
  refreshSaid: Said | null;
  onOpenRow: (index: number) => void;
  onRefresh: () => void;
}) {
  const nav = useRef<HTMLElement>(null);
  useEffect(() => {
    nav.current?.querySelector("[data-cursor]")?.scrollIntoView({ block: "nearest" });
  }, [cursor]);

  const byId = new Map(state?.sessions.map((s) => [s.id, s]));
  const focus: Entry[] = [];
  const lanes: { lane: SectionName | IssueLaneName; entries: Entry[] }[] = [];
  const issueLanes: typeof lanes = [];
  items.forEach((item, index) => {
    if (item.kind === "focus") {
      focus.push({ item, index });
      return;
    }
    const name = item.kind === "issue" ? item.lane : item.section;
    const into = item.kind === "issue" ? issueLanes : lanes;
    const lane = into.at(-1);
    if (lane?.lane === name) lane.entries.push({ item, index });
    else into.push({ lane: name, entries: [{ item, index }] });
  });
  const board = state?.board;
  const rest = [board?.idle && `${board.idle} idle`, board?.stopped && `${board.stopped} stopped`]
    .filter(Boolean)
    .join(" · ");

  const row = ({ item, index }: Entry) => {
    const to = itemRoute(item);
    const s = item.kind === "issue" ? undefined : byId.get(item.sessionId);
    if (item.kind !== "issue" && !s) return null;
    return (
      <RouteLink
        key={itemKey(item)}
        to={to}
        onGo={() => onOpenRow(index)}
        className={item.kind === "focus" ? "row focus-row" : "row lane-row"}
        data-cursor={itemKey(item) === cursor || undefined}
        aria-current={routePath(to) === openPath ? "page" : undefined}
      >
        {item.kind === "issue" ? (
          <IssueRow issue={item.issue} />
        ) : item.kind === "focus" ? (
          <FocusRow n={item.n} s={s!} />
        ) : (
          <LaneRow item={item} s={s!} now={now} />
        )}
      </RouteLink>
    );
  };
  const lane = ({ lane, entries }: (typeof lanes)[number]) => (
    <section key={lane} aria-labelledby={`lane-${lane}`}>
      <h2 id={`lane-${lane}`} className={`caps lane-title lane-${lane}`}>
        {SECTION_TITLE[lane]} {entries.length}
      </h2>
      {entries.map(row)}
    </section>
  );

  return (
    <div className="side">
      {state && <RefreshBar state={state} now={now} said={refreshSaid} onRefresh={onRefresh} />}
      <nav className="board" aria-label="Board" ref={nav}>
        {!state && <p className="side-note">Connecting…</p>}
        {state && (
          <section aria-labelledby="focus-title">
            <div className="list-head">
              <h2 id="focus-title" className="caps accent">
                Focus
              </h2>
              <span className="muted">
                {state.focus.source === "rule" ? "by rule" : state.focus.at ? clock(state.focus.at) : ""}
              </span>
            </div>
            {focus.length > 0 ? focus.map(row) : <p className="side-note">Nothing needs you.</p>}
          </section>
        )}
        {lanes.map(lane)}
        {rest && <p className="side-note">{rest}</p>}
        {issueLanes.length > 0 && <div className="issue-lanes">{issueLanes.map(lane)}</div>}
      </nav>
      <Ask disabled={!state?.orchestratorId} />
      <p className="hints">
        j/k move · ↵ open{state?.tui ? " · t terminal" : ""} · 1-9 answer · esc today
      </p>
    </div>
  );
}

/// When Linear's issues and the pull requests were read, Refresh, and why
/// part of the last read failed.
function RefreshBar({
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
        <span className="muted">{refreshed(refresh, now)}</span>
        <span className="sr" role="status">
          {heard}
        </span>
        <span className="flash-anchor">
          <button
            type="button"
            className="btn"
            onClick={onRefresh}
            aria-busy={refresh.running || undefined}
            aria-keyshortcuts="r"
            title="Reads Linear's issues assigned to you and the sessions' pull requests again."
          >
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

function FocusRow({ n, s }: { n: number; s: Session }) {
  return (
    <>
      <span className={n === 1 ? "badge first" : "badge"}>{n}</span>
      <span className="key">{sessionIssue(s)?.key ?? s.projectName}</span>
      <span className="title">{s.name}</span>
      <Dot tone={statusLook(s).tone} />
    </>
  );
}

function IssueRow({ issue }: { issue: Issue }) {
  return (
    <>
      <span className="key">{issue.key}</span>
      <span className="title" title={issue.title}>
        {issue.title}
      </span>
      <Priority p={issue.priority} />
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

function LaneRow({ item, s, now }: { item: ListItem & { kind: "row" }; s: Session; now: number }) {
  const workers = item.workers === 1 ? "1 worker" : `${item.workers} workers`;
  const text = s.why ?? (item.captain ? `${s.name} · ${workers}` : s.name);
  return (
    <>
      <span className="project">{s.projectName}</span>
      <span className="title" title={text}>
        {text}
      </span>
      <span className="age">{age(s.statusChangedAt, now)}</span>
    </>
  );
}

function Ask({ disabled }: { disabled: boolean }) {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const t = text.trim();
    if (!t || busy) return;
    setBusy(true);
    const err = await api.orchestrator(t);
    setBusy(false);
    setError(err);
    if (!err) setText("");
  };
  return (
    <form className="ask" onSubmit={submit}>
      <div className="ask-line">
        <label htmlFor="ask-input" className="accent">
          orchestrator ›
        </label>
        <input
          id="ask-input"
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder={disabled ? "No orchestrator yet" : "Ask, or hand out work"}
          disabled={disabled}
          autoComplete="off"
        />
        <button type="submit" className="btn" disabled={disabled || busy || !text.trim()}>
          Send <span aria-hidden="true">⏎</span>
        </button>
      </div>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </form>
  );
}
