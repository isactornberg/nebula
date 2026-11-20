import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { api } from "./api/client";
import type { Schedule, ScheduleRun, Schedules, ScheduleTarget, Session, State, Weekday } from "./api/contract";
import { look, type Route } from "./model";
import {
  addOf,
  blankDraft,
  DAYS,
  dayTime,
  hasRank,
  lastRuns,
  MORNING_RANK,
  nextLabel,
  OUTCOME_WORD,
  outcomeSaid,
  outcomeTone,
  scheduleLine,
  TARGET_TITLE,
  targetRefused,
  WEEKDAYS,
  whenLabel,
  type Draft,
} from "./schedules";
import { arrowsMove, Dot, Icon, RouteLink } from "./ui";

/// Which of the page's screens is up: the list with the runs beside it,
/// the add form, or one schedule.
export type ScheduleView = { kind: "list" } | { kind: "new" } | { kind: "one"; id: string };

/// What an act said, where it was asked.
type Said = { at: string; text: string; error: boolean };

const TARGETS: ScheduleTarget[] = ["newWorktree", "root", "session", "freshSession"];
/// A run's outcome as a pill, in the colour of its dot.
const OUTCOME_TINT: Record<ScheduleRun["outcome"], string> = {
  started: "tint-green",
  typed: "tint-green",
  restarted: "tint-green",
  held: "tint-yellow",
  failed: "tint-red",
  skipped: "tint-plain",
};

const DAY_LETTER: Record<Weekday, string> = {
  mon: "Mon",
  tue: "Tue",
  wed: "Wed",
  thu: "Thu",
  fri: "Fri",
  sat: "Sat",
  sun: "Sun",
};

export function SchedulesPage({
  state,
  view,
  now,
  onGo,
}: {
  state: State | null;
  view: ScheduleView;
  now: number;
  onGo: (to: Route, replace?: boolean) => void;
}) {
  const book = state?.schedules ?? null;
  const [said, setSaid] = useState<Said | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  /// A schedule just added, until the state carries it.
  const [added, setAdded] = useState<string | null>(null);
  const toList = () => onGo({ kind: "schedules" });

  useEffect(() => {
    if (view.kind === "list") return;
    const onKey = (e: KeyboardEvent) => {
      const t = e.target instanceof HTMLElement ? e.target : null;
      if (e.key !== "Escape" || e.defaultPrevented || (t && t.matches("input, textarea, select"))) return;
      e.preventDefault();
      toList();
    };
    addEventListener("keydown", onKey);
    return () => removeEventListener("keydown", onKey);
  });

  const addRank = () => {
    if (busy) return;
    setBusy("rank");
    void api.addSchedule(MORNING_RANK).then((r) => {
      setBusy(null);
      if (r.ok) {
        setAdded(r.value.id);
        setSaid({ at: r.value.id, text: "Added the morning rank.", error: false });
        onGo({ kind: "schedule", id: r.value.id });
      } else {
        setSaid({ at: "rank", text: r.error, error: true });
      }
    });
  };

  const one = view.kind === "one" ? (book?.schedules.find((s) => s.id === view.id) ?? null) : null;
  let pane;
  if (!state || !book) {
    pane = (
      <main className="pane idle">
        <p className="muted">Connecting…</p>
      </main>
    );
  } else if (view.kind === "new") {
    pane = (
      <AddForm
        state={state}
        book={book}
        now={now}
        onBack={toList}
        onAdded={(id, text) => {
          setAdded(id);
          setSaid({ at: id, text, error: false });
          onGo({ kind: "schedule", id });
        }}
      />
    );
  } else if (view.kind === "one" && one) {
    pane = (
      <OneSchedule
        key={one.id}
        schedule={one}
        state={state}
        book={book}
        now={now}
        said={said?.at === one.id ? said : null}
        busy={busy === one.id}
        onBack={toList}
        onGo={onGo}
        onRun={() => {
          setBusy(one.id);
          void api.runSchedule(one.id).then((r) => {
            setBusy(null);
            setSaid(
              r.ok
                ? { at: one.id, text: ranSaid(r.value, one.task === "rank"), error: r.value.outcome === "failed" }
                : { at: one.id, text: r.error, error: true },
            );
          });
        }}
        onRemove={() => {
          setBusy(one.id);
          void api.removeSchedule(one.id).then((err) => {
            setBusy(null);
            if (err) return setSaid({ at: one.id, text: err, error: true });
            setSaid({ at: "list", text: `Removed ${one.name}.`, error: false });
            toList();
          });
        }}
      />
    );
  } else if (view.kind === "one") {
    pane = (
      <main className="pane detail">
        <div className="detail-top">
          <RouteLink to={{ kind: "schedules" }} onGo={toList} className="back">
            <Icon name="chevronLeft" size={18} />
            Schedules
          </RouteLink>
        </div>
        <p className="muted">{added === view.id ? "Adding…" : "There is no schedule with that id."}</p>
      </main>
    );
  } else {
    pane = <Overview state={state} book={book} now={now} said={said?.at === "list" ? said : null} onGo={onGo} />;
  }

  return (
    <>
      <Side
        state={state}
        book={book}
        now={now}
        openId={view.kind === "one" ? view.id : null}
        adding={view.kind === "new"}
        busy={busy === "rank"}
        said={said?.at === "rank" ? said : null}
        onAddRank={addRank}
        onGo={onGo}
      />
      {pane}
    </>
  );
}

function ranSaid(run: ScheduleRun, rank: boolean): string {
  const word = OUTCOME_WORD[run.outcome];
  const said = outcomeSaid(run, rank);
  return `${word}${said ? `: ${said}` : ""}.`.replace("..", ".");
}

function Side({
  state,
  book,
  now,
  openId,
  adding,
  busy,
  said,
  onAddRank,
  onGo,
}: {
  state: State | null;
  book: Schedules | null;
  now: number;
  openId: string | null;
  adding: boolean;
  busy: boolean;
  said: Said | null;
  onAddRank: () => void;
  onGo: (to: Route) => void;
}) {
  const last = useMemo(() => lastRuns(book?.runs ?? []), [book?.runs]);
  return (
    <aside className="side sched-side" aria-label="Schedules">
      <div className="list-top">
        <h2 id="schedules-title">Schedules</h2>
        {book && book.schedules.length > 0 && (
          <span className="list-count">
            {book.schedules.length === 1 ? "1 schedule" : `${book.schedules.length} schedules`}
          </span>
        )}
        {book?.available && (
          <RouteLink
            to={{ kind: "scheduleNew" }}
            onGo={() => onGo({ kind: "scheduleNew" })}
            className="quiet-btn sched-add"
            aria-current={adding ? "page" : undefined}
          >
            <Icon name="plus" size={14} />
            Add
          </RouteLink>
        )}
      </div>
      <div className="board">
        <section aria-labelledby="schedules-title">
          {!state || !book ? null : !book.available ? (
            <p className="side-note">
              The running daemon keeps no schedules: <code>nebula reload</code> moves it onto this build, its sessions kept.
            </p>
          ) : book.schedules.length === 0 ? (
            <p className="side-note">No schedules yet.</p>
          ) : (
            <ul className="sched-rows">
              {book.schedules.map((s) => {
                const run = last.get(s.id);
                // A run whose session waits on you is in its colour, as that session's row is.
                const went = run?.sessionId ? state.sessions.find((x) => x.id === run.sessionId) : undefined;
                const waits = went && (look(went).tone === "red" || look(went).tone === "peach") ? look(went) : null;
                return (
                  <li key={s.id}>
                    <RouteLink
                      to={{ kind: "schedule", id: s.id }}
                      onGo={() => onGo({ kind: "schedule", id: s.id })}
                      className="row sched-row"
                      aria-current={s.id === openId ? "page" : undefined}
                    >
                      <span className="sched-dot" title={waits ? `Its session: ${waits.label.toLowerCase()}` : undefined}>
                        {waits ? (
                          <span aria-hidden="true" className={`sdot ${waits.tone}`} />
                        ) : (
                          <Dot tone={run ? outcomeTone(run.outcome) : "hollow"} />
                        )}
                      </span>
                      <span className="title-stack">
                        <span className="title">{s.name}</span>
                        <span className="sched-line">
                          {s.task === "rank" ? "orchestrator" : (s.projectName ?? "a project gone")} ·{" "}
                          {scheduleLine(s, now)}
                        </span>
                        <span className="sched-line">{nextLabel(s, now)}</span>
                      </span>
                    </RouteLink>
                  </li>
                );
              })}
            </ul>
          )}
        </section>
        {book?.available && !hasRank(book.schedules) && (
          <section className="sched-suggest" aria-labelledby="rank-title">
            <p>
              <strong id="rank-title">Morning rank</strong>
              <span className="muted">
                {" "}
                each weekday at 07:30: the orchestrator ranks the focus picks before you sit down.
              </span>
            </p>
            <div className="confirm-acts">
              <button type="button" className="btn" aria-busy={busy || undefined} disabled={busy} onClick={onAddRank}>
                Add the morning rank
              </button>
            </div>
            {said && (
              <p className={said.error ? "error" : "ok"} role="status">
                {said.text}
              </p>
            )}
          </section>
        )}
      </div>
    </aside>
  );
}

function Overview({
  state,
  book,
  now,
  said,
  onGo,
}: {
  state: State;
  book: Schedules;
  now: number;
  said: Said | null;
  onGo: (to: Route) => void;
}) {
  const next = book.schedules
    .filter((s) => s.nextAt !== null)
    .sort((a, b) => a.nextAt! - b.nextAt!)[0];
  const count = book.schedules.length;
  return (
    <main className="pane idle sched-overview">
      <div className="intro">
        <h1>Schedules</h1>
        <p className="muted">
          {!book.available
            ? "The running daemon keeps no schedules."
            : count === 0
              ? "Nothing runs on a schedule yet. A schedule runs a prompt in a project, or the orchestrator's morning rank, with no page or TUI open."
              : `${count === 1 ? "One schedule" : `${count} schedules`}${next ? `: next ${next.name}, ${dayTime(next.nextAt!, now)}.` : ", none due again."}`}
        </p>
      </div>
      {said && (
        <p className={said.error ? "note error" : "note"} role="status">
          {said.text}
        </p>
      )}
      {book.loginItem === false && book.available && (
        <p className="note sched-login">
          Schedules run while nebula's daemon runs, which after a reboot is once a TUI starts it.{" "}
          <code>nebula login-item on</code> starts it when you log in.
        </p>
      )}
      <section className="sched-runs" aria-labelledby="runs-title">
        <h2 id="runs-title" className="caps">
          Recent runs
        </h2>
        {book.runs.length === 0 ? (
          <p className="quiet">No runs yet.</p>
        ) : (
          <RunList runs={book.runs.slice(0, 40)} state={state} now={now} named onGo={onGo} />
        )}
      </section>
    </main>
  );
}

function RunList({
  runs,
  state,
  now,
  named,
  onGo,
}: {
  runs: ScheduleRun[];
  state: State;
  now: number;
  /// Each run says its schedule's name.
  named?: boolean;
  onGo: (to: Route) => void;
}) {
  const book = state.schedules;
  return (
    <ul className="runs">
      {runs.map((r) => {
        const s = book.schedules.find((x) => x.id === r.scheduleId);
        const session = r.sessionId ? state.sessions.find((x) => x.id === r.sessionId) : undefined;
        return (
          <li key={`${r.scheduleId}:${r.at}:${r.dueAt}`} className="run">
            <Dot tone={outcomeTone(r.outcome)} />
            <span className="run-at" title={new Date(r.at).toLocaleString()}>
              {dayTime(r.dueAt ?? r.at, now)}
            </span>
            <span className={`run-word tag ${OUTCOME_TINT[r.outcome]}`}>{OUTCOME_WORD[r.outcome]}</span>
            <span className="run-what">
              {named && s && (
                <>
                  <RouteLink to={{ kind: "schedule", id: s.id }} onGo={() => onGo({ kind: "schedule", id: s.id })}>
                    {s.name}
                  </RouteLink>
                  {" · "}
                </>
              )}
              {r.dueAt === null && <span className="quiet">run now · </span>}
              <RunSaid run={r} rank={s?.task === "rank"} session={session} onGo={onGo} />
              {r.note && <span className="run-note">{r.note}.</span>}
            </span>
          </li>
        );
      })}
    </ul>
  );
}

function RunSaid({
  run,
  rank,
  session,
  onGo,
}: {
  run: ScheduleRun;
  rank: boolean;
  session: Session | undefined;
  onGo: (to: Route) => void;
}) {
  const said = outcomeSaid(run, rank);
  if (!run.sessionId) return <span className="run-why">{said}</span>;
  const l = session ? look(session) : null;
  return (
    <span>
      {said}
      {" · "}
      {session ? (
        <RouteLink
          to={{ kind: "session", id: session.id }}
          onGo={() => onGo({ kind: "session", id: session.id })}
          className="run-session"
          title={l?.label}
        >
          <span aria-hidden="true" className={`sdot ${l!.tone}`} />
          {session.name}
        </RouteLink>
      ) : (
        <span className="quiet">a session no longer here</span>
      )}
    </span>
  );
}

function OneSchedule({
  schedule: s,
  state,
  book,
  now,
  said,
  busy,
  onBack,
  onGo,
  onRun,
  onRemove,
}: {
  schedule: Schedule;
  state: State;
  book: Schedules;
  now: number;
  said: Said | null;
  busy: boolean;
  onBack: () => void;
  onGo: (to: Route) => void;
  onRun: () => void;
  onRemove: () => void;
}) {
  const [removing, setRemoving] = useState(false);
  const keep = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (removing) keep.current?.focus();
  }, [removing]);
  const runs = book.runs.filter((r) => r.scheduleId === s.id);
  const session = s.sessionId ? state.sessions.find((x) => x.id === s.sessionId) : undefined;
  const rank = s.task === "rank";
  const rows: [string, React.ReactNode][] = rank
    ? [
        ["Runs", "The orchestrator's rank of the focus picks, started nebula serve first when none runs"],
        ["When", `${whenLabel(s.when, now)}, on this Mac's clock`],
      ]
    : [
        ["Project", s.projectName ?? "a project gone from nebula"],
        ["Where", TARGET_TITLE[s.target!]],
        ["Harness", book.harnesses.find((h) => h.id === s.harness)?.label ?? s.harness],
        ["Model", s.model ?? "the CLI's own"],
        ["Effort", s.effort ?? "the CLI's own"],
        ...(s.harness === "claude"
          ? ([["Permission mode", s.permissionMode ?? "default, your Claude settings'"]] as [string, string][])
          : []),
        ["When", `${whenLabel(s.when, now)}${s.when.kind === "at" ? ", on this Mac's clock" : ""}`],
      ];
  if (s.target === "session" || s.target === "freshSession") {
    rows.push([
      "Session",
      session ? (
        <RouteLink to={{ kind: "session", id: session.id }} onGo={() => onGo({ kind: "session", id: session.id })}>
          <span aria-hidden="true" className={`sdot ${look(session).tone}`} />
          {session.name}
        </RouteLink>
      ) : s.sessionId ? (
        "gone: the next run starts another"
      ) : (
        "the first run starts it"
      ),
    ]);
  }
  rows.push(["Added", dayTime(s.createdAt, now)]);
  return (
    <main className="pane detail sched-detail">
      <div className="detail-top">
        <RouteLink to={{ kind: "schedules" }} onGo={onBack} className="back">
          <Icon name="chevronLeft" size={18} />
          Schedules
        </RouteLink>
        {!removing && (
          <div className="actions">
            <button type="button" className="btn" aria-busy={busy || undefined} disabled={busy} onClick={onRun}>
              Run now
            </button>
            <button type="button" className="btn quiet-danger" disabled={busy} onClick={() => setRemoving(true)}>
              Remove
            </button>
          </div>
        )}
      </div>
      {removing && (
        <div className="fix-confirm delete-confirm" role="group" aria-labelledby="remove-ask">
          <p id="remove-ask">
            Remove {s.name}? It runs no more, and its {runs.length === 1 ? "run" : `${runs.length} runs`} go with it.
            The sessions it started stay.
          </p>
          <div className="confirm-acts">
            <button type="button" className="btn danger" aria-busy={busy || undefined} disabled={busy} onClick={onRemove}>
              Remove {s.name}
            </button>
            <button ref={keep} type="button" className="fix-act" onClick={() => setRemoving(false)}>
              Cancel
            </button>
          </div>
        </div>
      )}
      {said && (
        <p className={said.error ? "error" : "ok"} role="status">
          {said.text}
        </p>
      )}
      <div className="title-block">
        <h1>{s.name}</h1>
        <p className="why">{s.nextAt === null ? "Done: it runs no more." : `Runs ${nextLabel(s, now)}.`}</p>
      </div>
      {s.prompt && <pre className="sched-prompt">{s.prompt}</pre>}
      <table className="fields sched-fields">
        <tbody>
          {rows.map(([k, v]) => (
            <tr key={k}>
              <th scope="row">{k}</th>
              <td>{v}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <section className="sched-runs" aria-labelledby="its-runs">
        <h2 id="its-runs" className="caps">
          Runs{runs.length > 0 ? ` ${runs.length}` : ""}
        </h2>
        {runs.length === 0 ? (
          <p className="quiet">None yet.</p>
        ) : (
          <RunList runs={runs} state={state} now={now} onGo={onGo} />
        )}
      </section>
    </main>
  );
}

function AddForm({
  state,
  book,
  now,
  onBack,
  onAdded,
}: {
  state: State;
  book: Schedules;
  now: number;
  onBack: () => void;
  onAdded: (id: string, said: string) => void;
}) {
  const [d, setD] = useState<Draft>(() => blankDraft(state.projects[0]?.id ?? ""));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const set = (patch: Partial<Draft>) => {
    setD((was) => ({ ...was, ...patch }));
    setError(null);
  };
  const harness = book.harnesses.find((h) => h.id === d.harness);
  const refused = targetRefused(d.target, harness);
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (busy) return;
    const add = addOf(d, Date.now());
    if (typeof add === "string") return setError(add);
    if (refused) return setError(refused);
    setBusy(true);
    void api.addSchedule(add).then((r) => {
      setBusy(false);
      if (r.ok) onAdded(r.value.id, "Added.");
      else setError(r.error);
    });
  };
  const toggleDay = (day: Weekday) =>
    set({ days: d.days.includes(day) ? d.days.filter((x) => x !== day) : DAYS.filter((x) => x === day || d.days.includes(x)) });
  const models = harness?.models ?? [];
  const efforts = harness?.efforts ?? [];
  return (
    <main className="pane detail sched-new">
      <div className="detail-top">
        <RouteLink to={{ kind: "schedules" }} onGo={onBack} className="back">
          <Icon name="chevronLeft" size={18} />
          Schedules
        </RouteLink>
      </div>
      <div className="title-block">
        <h1>Add a schedule</h1>
        <p className="why">The daemon runs it on its own, with no page or TUI open, on this Mac's clock.</p>
      </div>
      <form className="sched-form" onSubmit={submit} noValidate>
        <div className="field">
          <span className="field-label" id="task-label">
            Runs
          </span>
          <div role="group" aria-labelledby="task-label" className="positions" onKeyDown={arrowsMove}>
            {(["prompt", "rank"] as const).map((t) => (
              <button key={t} type="button" aria-pressed={d.task === t} onClick={() => set({ task: t })}>
                {t === "prompt" ? "A prompt" : "The morning rank"}
              </button>
            ))}
          </div>
        </div>
        {d.task === "rank" ? (
          <p className="field-help">
            The orchestrator ranks the web app's focus picks, as Re-rank asks it to, with nebula serve started first
            when none runs.
          </p>
        ) : (
          <>
            <label className="field">
              <span className="field-label">Project</span>
              <select value={d.projectId} onChange={(e) => set({ projectId: e.target.value })}>
                {state.projects.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </select>
            </label>
            <label className="field wide">
              <span className="field-label">Prompt</span>
              <textarea
                rows={4}
                value={d.prompt}
                placeholder="What each run starts on"
                onChange={(e) => set({ prompt: e.target.value })}
              />
            </label>
            <label className="field wide">
              <span className="field-label">Where</span>
              <select value={d.target} onChange={(e) => set({ target: e.target.value as ScheduleTarget })}>
                {TARGETS.map((t) => (
                  <option key={t} value={t} disabled={targetRefused(t, harness) !== null}>
                    {TARGET_TITLE[t]}
                  </option>
                ))}
              </select>
              <span className="field-help">
                {refused ??
                  (d.target === "session"
                    ? "The first run starts it in the root checkout; each later run waits until it is at rest with an empty input box."
                    : d.target === "freshSession"
                      ? "Its CLI starts again on a new conversation for each later run, once it is at rest with an empty input box."
                      : d.target === "newWorktree"
                        ? "Each run cuts a worktree of its own, named for the schedule and when it ran."
                        : "Each run starts beside whatever else runs there.")}
              </span>
            </label>
            <div className="field-row">
              <label className="field">
                <span className="field-label">Harness</span>
                <select
                  value={d.harness}
                  onChange={(e) => set({ harness: e.target.value, model: "", effort: "", permissionMode: "" })}
                >
                  {book.harnesses.map((h) => (
                    <option key={h.id} value={h.id}>
                      {h.label}
                    </option>
                  ))}
                </select>
              </label>
              <label className="field">
                <span className="field-label">Model</span>
                <input
                  value={d.model}
                  list="sched-models"
                  placeholder="the CLI's own"
                  onChange={(e) => set({ model: e.target.value })}
                />
                <datalist id="sched-models">
                  {models.map((m) => (
                    <option key={m} value={m} />
                  ))}
                </datalist>
              </label>
              <label className="field">
                <span className="field-label">Effort</span>
                <input
                  value={d.effort}
                  list="sched-efforts"
                  placeholder="the CLI's own"
                  onChange={(e) => set({ effort: e.target.value })}
                />
                <datalist id="sched-efforts">
                  {efforts.map((m) => (
                    <option key={m} value={m} />
                  ))}
                </datalist>
              </label>
              <label className="field">
                <span className="field-label">Permission mode</span>
                <select
                  value={d.harness === "claude" ? d.permissionMode : ""}
                  disabled={d.harness !== "claude"}
                  title={d.harness === "claude" ? undefined : "Claude's --permission-mode alone"}
                  onChange={(e) => set({ permissionMode: e.target.value })}
                >
                  <option value="">{d.harness === "claude" ? "default" : "as nebula launches it"}</option>
                  {d.harness === "claude" &&
                    book.permissionModes.map((m) => (
                      <option key={m} value={m}>
                        {m}
                      </option>
                    ))}
                </select>
              </label>
            </div>
            {d.harness === "claude" && (d.permissionMode === "" || d.permissionMode === "manual") && (
              <p className="field-help">
                A run that asks a permission waits on its dialog, red on the board, until you answer it; manual asks for
                every one, whatever your Claude settings say.
              </p>
            )}
          </>
        )}
        <div className="field">
          <span className="field-label" id="when-label">
            When
          </span>
          <div role="group" aria-labelledby="when-label" className="positions" onKeyDown={arrowsMove}>
            {(["every", "at", "once"] as const).map((k) => (
              <button key={k} type="button" aria-pressed={d.whenKind === k} onClick={() => set({ whenKind: k })}>
                {k === "every" ? "Every N minutes" : k === "at" ? "At a time of day" : "Once"}
              </button>
            ))}
          </div>
        </div>
        {d.whenKind === "every" && (
          <label className="field inline">
            <span className="field-label">Every</span>
            <input
              type="number"
              min={1}
              inputMode="numeric"
              value={d.minutes}
              onChange={(e) => set({ minutes: e.target.value })}
            />
            <span className="muted">minutes, the first that long from now</span>
          </label>
        )}
        {d.whenKind === "at" && (
          <div className="field-row">
            <label className="field">
              <span className="field-label">At</span>
              <input type="time" value={d.time} onChange={(e) => set({ time: e.target.value })} />
            </label>
            <div className="field">
              <span className="field-label" id="days-label">
                On
              </span>
              <div className="days-line">
                <div role="group" aria-labelledby="days-label" className="positions days" onKeyDown={arrowsMove}>
                  {DAYS.map((day) => (
                    <button key={day} type="button" aria-pressed={d.days.includes(day)} onClick={() => toggleDay(day)}>
                      {DAY_LETTER[day]}
                    </button>
                  ))}
                </div>
                <div className="day-sets">
                  <button type="button" className="fix-act" onClick={() => set({ days: WEEKDAYS })}>
                    Weekdays
                  </button>
                  <button type="button" className="fix-act" onClick={() => set({ days: DAYS })}>
                    Daily
                  </button>
                </div>
              </div>
            </div>
          </div>
        )}
        {d.whenKind === "once" && (
          <label className="field">
            <span className="field-label">On</span>
            <input
              type="datetime-local"
              value={d.once}
              min={localInput(now)}
              onChange={(e) => set({ once: e.target.value })}
            />
          </label>
        )}
        {d.whenKind !== "every" && (
          <p className="field-help">
            {d.whenKind === "at" && d.days.length === 7
              ? "A run the Mac slept through runs once when it wakes, that day, saying so."
              : "A run the Mac slept through more than ten minutes is skipped, and the list says why."}
          </p>
        )}
        <label className="field wide">
          <span className="field-label">Name</span>
          <input
            value={d.name}
            placeholder={d.task === "rank" ? "Morning rank" : "the prompt's first words"}
            onChange={(e) => set({ name: e.target.value })}
          />
        </label>
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <div className="confirm-acts">
          <button type="submit" className="btn primary" aria-busy={busy || undefined} disabled={busy}>
            Add schedule
          </button>
          <button type="button" className="fix-act" onClick={onBack}>
            Cancel
          </button>
        </div>
      </form>
    </main>
  );
}

/// `at` as a `datetime-local` input takes it.
function localInput(at: number): string {
  const d = new Date(at);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}
