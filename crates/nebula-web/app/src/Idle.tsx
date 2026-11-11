import type { State } from "./api/contract";
import { cardAnswers, itemKey, longDate, prLabel, statusLook, summary, type FocusItem, type ListItem } from "./model";
import { Dot, External, RouteLink } from "./ui";

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
  onAnswer,
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
  onAnswer: (item: FocusItem, n: number) => void;
}) {
  const picks = items.flatMap((i) => (i.kind === "focus" ? [i] : []));
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
      {picks.length > 0 ? (
        <section className="cards" aria-label="Focus">
          {picks.map((p) => {
            const s = state.sessions.find((x) => x.id === p.sessionId)!;
            const answers = cardAnswers(p, s);
            return (
              <article key={p.sessionId} className="card" data-cursor={itemKey(p) === cursor || undefined}>
                <div className="card-head">
                  <span className={p.n === 1 ? "badge big first" : "badge big"}>{p.n}</span>
                  {s.issueKey && s.issueUrl && (
                    <External href={s.issueUrl} className="muted-link">
                      {s.issueKey}
                    </External>
                  )}
                  <span className="card-where">
                    <Dot tone={statusLook(s).tone} />
                    <span className="clip">{s.projectName}</span>
                  </span>
                </div>
                <h2>{s.name}</h2>
                {p.reason && <p className="reason">{p.reason}</p>}
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
                  {s.prUrl && <External href={s.prUrl}>{prLabel(s.prUrl)} ↗</External>}
                </div>
              </article>
            );
          })}
        </section>
      ) : (
        <p className="calm">Nothing needs you.</p>
      )}
    </main>
  );
}
