import { useEffect, useRef, useState, type FormEvent } from "react";
import type { State, Verdict } from "./api/contract";
import {
  age,
  cardAnswers,
  handedLine,
  itemKey,
  longDate,
  noPicks,
  prLabel,
  prStateLabel,
  sessionIssue,
  stateTone,
  statusLook,
  summary,
  unpicked,
  type FocusItem,
  type ListItem,
} from "./model";
import { LaneRow, Priority } from "./Side";
import { TodayBand } from "./Today";
import { Dot, External, RouteLink } from "./ui";

/// Puts a pick aside, `verdict` with an optional reason: null once done,
/// else why not.
export type SayOf = (item: FocusItem, verdict: Verdict, reason: string | null) => Promise<string | null>;

export function Idle({
  state,
  items,
  cursor,
  now,
  note,
  busy,
  error,
  actedOn,
  onOpen,
  onOpenIssue,
  onAnswer,
  onSay,
}: {
  state: State;
  items: ListItem[];
  cursor: string | null;
  now: number;
  note: string | null;
  busy: boolean;
  error: string | null;
  actedOn: string | null;
  onOpen: (id: string) => void;
  onOpenIssue: (key: string) => void;
  onAnswer: (item: FocusItem, n: number) => void;
  onSay: SayOf;
}) {
  const picks = items.flatMap((i) => (i.kind === "focus" ? [i] : []));
  const byOrchestrator = state.focus.source === "orchestrator";
  // By the orchestrator, the rows that need you and that it left out stay
  // in sight, so the page never reads as if nothing needed you.
  const left = byOrchestrator ? unpicked(items) : [];
  return (
    <main className="pane idle">
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      <div className="intro">
        <h1>{longDate(now)}</h1>
        <p className="muted">{summary(state, picks.length)}</p>
      </div>
      {picks.length > 0 && (
        <section className="cards" aria-label="Focus">
          {picks.map((p) => {
            const cursorOn = itemKey(p) === cursor || undefined;
            const say = byOrchestrator ? <PickFeedback item={p} onSay={onSay} /> : null;
            if (p.issue) {
              const i = p.issue;
              return (
                <article key={p.key} className="card" data-cursor={cursorOn}>
                  <div className="card-head">
                    <span className={p.n === 1 ? "badge big first" : "badge big"}>{p.n}</span>
                    <External href={i.url} className="muted-link">
                      {i.key}
                    </External>
                    <span className="card-where">
                      <Dot tone={stateTone(i.stateType)} />
                      <span className="clip">{i.state}</span>
                    </span>
                  </div>
                  <h2>{i.title}</h2>
                  {p.reason && <p className="reason">{p.reason}</p>}
                  {say}
                  <div className="card-foot">
                    <RouteLink
                      to={{ kind: "issue", key: i.key }}
                      onGo={() => onOpenIssue(i.key)}
                      aria-label={`Open ${i.key}`}
                    >
                      Open ›
                    </RouteLink>
                    <span className="card-mark">
                      <Priority p={i.priority} />
                      {i.started && (
                        <span className="handed" title={handedLine(i.started, now)}>
                          {i.started.held ? "brief held" : `handed ${age(i.started.at, now)}`}
                        </span>
                      )}
                    </span>
                  </div>
                </article>
              );
            }
            const s = state.sessions.find((x) => x.id === p.sessionId)!;
            const answers = cardAnswers(p, s);
            const linked = sessionIssue(s);
            return (
              <article key={p.key} className="card" data-cursor={cursorOn}>
                <div className="card-head">
                  <span className={p.n === 1 ? "badge big first" : "badge big"}>{p.n}</span>
                  {linked && (
                    <External href={linked.url} className="muted-link">
                      {linked.key}
                    </External>
                  )}
                  <span className="card-where">
                    <Dot tone={statusLook(s).tone} />
                    <span className="clip">{s.projectName}</span>
                  </span>
                </div>
                <h2>{s.name}</h2>
                {p.reason && <p className="reason">{p.reason}</p>}
                {say}
                {answers.length > 0 && (
                  <div className="pills" role="group" aria-label={`Answers for ${s.name}`}>
                    {answers.map((a, i) => (
                      <button
                        key={a}
                        type="button"
                        className={i === 0 ? "pill first" : "pill"}
                        disabled={busy}
                        onClick={() => onAnswer(p, i + 1)}
                      >
                        {i + 1} {a}
                      </button>
                    ))}
                  </div>
                )}
                {error && actedOn === s.id && (
                  <p className="error" role="alert">
                    {error}
                  </p>
                )}
                <div className="card-foot">
                  <RouteLink
                    to={{ kind: "session", id: s.id }}
                    onGo={() => onOpen(s.id)}
                    aria-label={`Open ${s.name}`}
                  >
                    Open ›
                  </RouteLink>
                  {s.prUrl && (
                    <span>
                      <External href={s.prUrl}>{prLabel(s.prUrl)} ↗</External>
                      {s.prState && <span className={`pr-state pr-${s.prState}`}> · {prStateLabel(s.prState, true)}</span>}
                    </span>
                  )}
                </div>
              </article>
            );
          })}
        </section>
      )}
      {picks.length === 0 && left.length === 0 && <p className="calm">{noPicks(state)}</p>}
      {left.length > 0 && (
        <section className="left" aria-labelledby="left-title">
          <h2 id="left-title" className="left-head">
            <span className="caps lane-needsYou">{picks.length > 0 ? "Also needs you" : "Needs you"}</span>
            <span className="muted">
              {picks.length > 0 ? "not among the orchestrator's picks" : "the orchestrator picked none of these"}
            </span>
          </h2>
          <div className="left-rows">
            {left.map((item) => {
              const s = state.sessions.find((x) => x.id === item.sessionId);
              if (!s) return null;
              return (
                <RouteLink
                  key={item.sessionId}
                  to={{ kind: "session", id: s.id }}
                  onGo={() => onOpen(s.id)}
                  className="row lane-row"
                  data-cursor={itemKey(item) === cursor || undefined}
                >
                  <LaneRow item={item} s={s} now={now} />
                </RouteLink>
              );
            })}
          </div>
        </section>
      )}
      <TodayBand state={state} now={now} />
    </main>
  );
}

/// Not today and Wrong on one of the orchestrator's picks: each asks for
/// an optional reason on a row of its own before it puts the pick aside.
export function PickFeedback({ item, onSay }: { item: FocusItem; onSay: SayOf }) {
  const [asking, setAsking] = useState<Verdict | null>(null);
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const field = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (asking) field.current?.focus();
  }, [asking]);
  const close = () => {
    setAsking(null);
    setReason("");
    setError(null);
  };
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!asking || busy) return;
    setBusy(true);
    const err = await onSay(item, asking, reason.trim() || null);
    setBusy(false);
    setError(err);
  };
  if (!asking) {
    return (
      <span className="pick-say">
        <button type="button" className="link-btn" onClick={() => setAsking("notToday")}>
          Not today
        </button>
        <button type="button" className="link-btn" onClick={() => setAsking("wrong")}>
          Wrong
        </button>
      </span>
    );
  }
  return (
    <form
      className="pick-ask"
      onSubmit={submit}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          close();
        }
      }}
    >
      <label htmlFor={`say-${item.key}`}>{asking === "notToday" ? "Why not today?" : "Why is it wrong?"}</label>
      <input
        id={`say-${item.key}`}
        ref={field}
        value={reason}
        onChange={(e) => setReason(e.target.value)}
        placeholder="Optional, for the next rank"
        maxLength={300}
        autoComplete="off"
      />
      <span className="pick-acts">
        <button type="submit" className="btn primary" disabled={busy}>
          {asking === "notToday" ? "Not today" : "Wrong"}
        </button>
        <button type="button" className="btn" onClick={close}>
          Cancel
        </button>
      </span>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </form>
  );
}
