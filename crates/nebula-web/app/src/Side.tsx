import { useEffect, useRef, useState, type FormEvent } from "react";
import { api } from "./api/client";
import type { SectionName, Session, State } from "./api/contract";
import { age, clock, itemKey, statusLook, type ListItem } from "./model";
import { Dot, RouteLink } from "./ui";

const SECTION_TITLE: Record<SectionName, string> = {
  needsYou: "Needs you",
  done: "Done",
  running: "Running",
  captains: "Captains",
};

type Entry = { item: ListItem; index: number };

export function Side({
  state,
  items,
  cursor,
  openId,
  now,
  onOpenRow,
}: {
  state: State | null;
  items: ListItem[];
  cursor: string | null;
  openId: string | null;
  now: number;
  onOpenRow: (index: number) => void;
}) {
  const nav = useRef<HTMLElement>(null);
  useEffect(() => {
    nav.current?.querySelector("[data-cursor]")?.scrollIntoView({ block: "nearest" });
  }, [cursor]);

  const byId = new Map(state?.sessions.map((s) => [s.id, s]));
  const focus: Entry[] = [];
  const lanes: { section: SectionName; entries: Entry[] }[] = [];
  items.forEach((item, index) => {
    if (item.kind === "focus") {
      focus.push({ item, index });
      return;
    }
    const lane = lanes.at(-1);
    if (lane?.section === item.section) lane.entries.push({ item, index });
    else lanes.push({ section: item.section, entries: [{ item, index }] });
  });
  const board = state?.board;
  const rest = [board?.idle && `${board.idle} idle`, board?.stopped && `${board.stopped} stopped`]
    .filter(Boolean)
    .join(" · ");

  const row = ({ item, index }: Entry) => {
    const s = byId.get(item.sessionId);
    if (!s) return null;
    const lit = item.sessionId === openId;
    return (
      <RouteLink
        key={itemKey(item)}
        to={{ kind: "session", id: s.id }}
        onGo={() => onOpenRow(index)}
        className={item.kind === "focus" ? "row focus-row" : "row lane-row"}
        data-cursor={itemKey(item) === cursor || undefined}
        aria-current={lit ? "page" : undefined}
      >
        {item.kind === "focus" ? <FocusRow n={item.n} s={s} /> : <LaneRow item={item} s={s} now={now} />}
      </RouteLink>
    );
  };

  return (
    <div className="side">
      <nav className="board" aria-label="Board" ref={nav}>
        {!state && <p className="side-note">Connecting…</p>}
        {focus.length > 0 && state && (
          <section aria-labelledby="focus-title">
            <div className="list-head">
              <h2 id="focus-title" className="caps accent">
                Focus
              </h2>
              <span className="muted">
                {state.focus.source === "rule" ? "by rule" : state.focus.at ? clock(state.focus.at) : ""}
              </span>
            </div>
            {focus.map(row)}
          </section>
        )}
        {lanes.map((lane) => (
          <section key={lane.section} aria-labelledby={`lane-${lane.section}`}>
            <h2 id={`lane-${lane.section}`} className={`caps lane-title lane-${lane.section}`}>
              {SECTION_TITLE[lane.section]} {lane.entries.length}
            </h2>
            {lane.entries.map(row)}
          </section>
        ))}
        {state && items.length === 0 && <p className="side-note">Nothing needs you.</p>}
        {rest && <p className="side-note">{rest}</p>}
      </nav>
      <Ask disabled={!state?.orchestratorId} />
      <p className="hints">
        j/k move · ↵ open{state?.tui ? " · t terminal" : ""} · 1-9 answer · esc today
      </p>
    </div>
  );
}

function FocusRow({ n, s }: { n: number; s: Session }) {
  return (
    <>
      <span className={n === 1 ? "badge first" : "badge"}>{n}</span>
      <span className="key">{s.issueKey ?? s.projectName}</span>
      <span className="title">{s.name}</span>
      <Dot tone={statusLook(s).tone} />
    </>
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
          Send ⏎
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
