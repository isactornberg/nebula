import { useEffect, useId, useRef, useState, type FormEvent, type ReactNode } from "react";
import { useDetail } from "./api/client";
import type { Session, State, Verdict } from "./api/contract";
import {
  actingOn,
  age,
  ago,
  cardAnswers,
  comingUp,
  composing,
  dayLine,
  focusLine,
  focusNote,
  focusPicks,
  handedLine,
  longDate,
  look,
  NO_ORCHESTRATOR,
  noPicks,
  pickRoute,
  priorityLabel,
  prPill,
  ranking,
  rowMeta,
  sessionOrder,
  shippedToday,
  shownName,
  startOffer,
  stripReport,
  unpicked,
  TONE_TINT,
  type Acting,
  type FocusItem,
  type Route,
} from "./model";
import { dayTime } from "./schedules";
import { SessionRow } from "./Side";
import { nightlyChip, nightlyFailure, TodayTiles } from "./Today";
import { External, FlashSpot, Icon, RouteLink, type Said } from "./ui";

/// Puts a pick aside, `verdict` with an optional reason: null once done,
/// else why not.
export type SayOf = (item: FocusItem, verdict: Verdict, reason: string | null) => Promise<string | null>;

/// What Home and the Spotlight act with.
export interface HomeActs {
  onOpen: (id: string) => void;
  onOpenIssue: (key: string) => void;
  /// Start on an issue's card: its page, with Start's question open.
  onStart: (key: string) => void;
  onAnswer: (item: FocusItem, n: number) => void;
  /// Picks option `n` of the dialog session `id` shows.
  onOption: (id: string, n: number) => void;
  onSay: SayOf;
  onRank: () => void;
  onGo: (to: Route) => void;
}

/// Home, while nothing is open on a wide screen: the day, the Spotlight,
/// the day's tiles, what shipped today, and what comes up.
export function Home({
  state,
  now,
  note,
  cursor,
  acting,
  rankSaid,
  acts,
}: {
  state: State;
  now: number;
  note: ReactNode;
  /// The list's cursor: the card of the session or issue it is on is the
  /// one `1`-`9` answer.
  cursor: string | null;
  acting: Acting;
  rankSaid: Said | null;
  acts: HomeActs;
}) {
  return (
    <main className="home">
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      <header className="day">
        <div className="day-text">
          <h1>{longDate(now)}</h1>
          <p className="day-line">{dayLine(state.board)}</p>
        </div>
        <RankAgain state={state} said={rankSaid} onRank={acts.onRank} />
      </header>
      <Spotlight
        state={state}
        now={now}
        cursor={cursor}
        acting={acting}
        acts={acts}
        rank={null}
        left
      />
      <div className="home-grid">
        <TodayTiles state={state} now={now} />
        <Shipped state={state} now={now} onOpen={acts.onOpen} />
      </div>
      <ComingUp state={state} now={now} onGo={acts.onGo} />
    </main>
  );
}

/// Asks the orchestrator to pick again; it waits on a rank under way.
export function RankAgain({ state, said, onRank }: { state: State; said: Said | null; onRank: () => void }) {
  const busy = ranking(state);
  return (
    <span className="flash-anchor rank">
      <button
        type="button"
        className="btn"
        onClick={onRank}
        disabled={!state.orchestratorId || busy}
        aria-busy={busy || undefined}
        title={
          state.orchestratorId
            ? "Has the orchestrator choose and explain the picks again."
            : NO_ORCHESTRATOR
        }
      >
        <Icon name="refresh" size={15} />
        {busy ? "Ranking…" : "Rank again"}
      </button>
      <FlashSpot flash={said} />
    </span>
  );
}

/// The focus picks as cards, each answered from where it is: a session's
/// with the answers its reply offers, an issue's with Start; why the rule
/// stands in for the orchestrator; and what needs you that it left out.
export function Spotlight({
  state,
  now,
  cursor,
  acting,
  acts,
  rank,
  left: showLeft,
}: {
  state: State;
  now: number;
  cursor: string | null;
  acting: Acting;
  acts: HomeActs;
  /// Rank again, where the Spotlight heads the page.
  rank: ReactNode;
  /// Lists what needs you that the orchestrator left out, where the list
  /// is not right under it.
  left: boolean;
}) {
  const picks = focusPicks(state);
  const byOrchestrator = state.focus.source === "orchestrator";
  const note = focusNote(state, now);
  const line = focusLine(state, picks.length);
  // By the orchestrator, the rows that need you and that it left out stay
  // in sight, so the page never reads as if nothing needed you.
  const left = byOrchestrator && showLeft ? unpicked(state, picks) : [];
  const byId = new Map(state.sessions.map((s) => [s.id, s]));
  return (
    <section className="spot" aria-labelledby="spot-title">
      <div className="spot-head">
        <h2 id="spot-title">
          <Icon name="sparkle" size={15} className="accent" />
          Focus
        </h2>
        {line && <span className="home-sub">{line}</span>}
        {rank}
      </div>
      {note && <p className={state.focus.rank?.state === "ended" ? "spot-note late" : "spot-note"}>{note}</p>}
      {picks.length > 0 && (
        <div className="cards">
          {picks.map((p) => {
            const s = p.sessionId === null ? undefined : byId.get(p.sessionId);
            return (
              <SpotCard
                key={p.key}
                item={p}
                s={s}
                state={state}
                now={now}
                cursorOn={cursor === (s ? `row:${s.id}` : `issue:${p.key}`)}
                {...(s ? actingOn(acting, s.id) : NOTHING_ASKED)}
                acts={acts}
                feedback={byOrchestrator}
              />
            );
          })}
        </div>
      )}
      {picks.length === 0 && left.length === 0 && <p className="calm">{noPicks(state)}</p>}
      {left.length > 0 && (
        <section className="left" aria-labelledby="left-title">
          <h3 id="left-title" className="left-head">
            <span className="left-word">{picks.length > 0 ? "Also needs you" : "Needs you"}</span>
            <span className="home-sub">
              {picks.length > 0 ? "not among the orchestrator's picks" : "the orchestrator picked none of these"}
            </span>
          </h3>
          <div className="panel-rows">
            {left.map((item) => {
              const s = byId.get(item.sessionId);
              if (!s) return null;
              return (
                <RouteLink
                  key={item.sessionId}
                  to={{ kind: "session", id: s.id }}
                  onGo={() => acts.onOpen(s.id)}
                  className="srow"
                  data-cursor={cursor === `row:${s.id}` || undefined}
                >
                  <SessionRow s={s} workers={item.workers} now={now} />
                </RouteLink>
              );
            })}
          </div>
        </section>
      )}
    </section>
  );
}

const NOTHING_ASKED = { busy: false, error: null };

/// A pick's card. Its head ends in a way to put it aside, as Not now on an
/// issue's card does.
function SpotCard({
  item,
  s,
  state,
  now,
  cursorOn,
  busy,
  error,
  acts,
  feedback,
}: {
  item: FocusItem;
  s: Session | undefined;
  state: State;
  now: number;
  cursorOn: boolean;
  busy: boolean;
  error: string | null;
  acts: HomeActs;
  /// The orchestrator picked it, so it takes Not today and Wrong.
  feedback: boolean;
}) {
  const [say, setSay] = useState<Verdict | "menu" | null>(null);
  const issue = item.issue;
  const key = s ? (s.issue?.key ?? s.issueKey) : item.key;
  const keyUrl = s ? (s.issue?.url ?? s.issueUrl) : (issue?.url ?? null);
  const where = s ? s.projectName : (issue?.project ?? issue?.team ?? "");
  const title = s ? s.name : (issue?.title ?? item.key);
  const reason = s ? stripReport(item.reason) : item.reason;
  return (
    <article className="card" data-cursor={cursorOn || undefined}>
      <div className="card-head">
        <span className={item.n === 1 ? "card-n first" : "card-n"}>{item.n}</span>
        {key &&
          (keyUrl ? (
            <External href={keyUrl} className="card-key m">
              {key}
            </External>
          ) : (
            <span className="card-key m">{key}</span>
          ))}
        {key && where && <span aria-hidden="true">·</span>}
        <span className="card-where">{where}</span>
        {feedback && (
          <button
            type="button"
            className="card-aside"
            aria-label="Not today, or a wrong pick"
            aria-expanded={say !== null}
            onClick={() => setSay(say === null ? "menu" : null)}
          >
            <Icon name="more" size={16} />
          </button>
        )}
      </div>
      {say === "menu" && (
        <div className="card-say">
          <button type="button" className="link-btn" onClick={() => setSay("notToday")}>
            Not today
          </button>
          <button type="button" className="link-btn" onClick={() => setSay("wrong")}>
            Wrong
          </button>
        </div>
      )}
      {(say === "notToday" || say === "wrong") && (
        <PickAsk item={item} verdict={say} onSay={acts.onSay} onClose={() => setSay(null)} />
      )}
      <RouteLink
        to={pickRoute(item)}
        onGo={() => (s ? acts.onOpen(s.id) : acts.onOpenIssue(item.key))}
        className="card-title"
      >
        {title}
      </RouteLink>
      {s ? <SessionState s={s} state={state} now={now} reason={item.reason} /> : issue && <IssueState item={item} now={now} />}
      {reason && <p className="card-why">{reason}</p>}
      {s?.role === "captain" && <Workers captain={s} state={state} now={now} />}
      {!s && issue && !issue.started && <StartsIn item={item} state={state} />}
      {s?.status === "needsFeedback" ? (
        <DialogActs s={s} busy={busy} onOption={acts.onOption} />
      ) : (
        <CardActs
          item={item}
          s={s}
          busy={busy}
          onAnswer={acts.onAnswer}
          onStart={acts.onStart}
          onNotNow={feedback ? () => setSay("notToday") : null}
        />
      )}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </article>
  );
}

/// A session card's state in a pill, how long it has stood, and beside it
/// a captain's workers or the dialog it shows.
function SessionState({ s, state, now, reason }: { s: Session; state: State; now: number; reason: string }) {
  const l = look(s);
  const pr = prPill(s);
  const workers = s.role === "captain" ? captainWorkers(s, state).length : 0;
  const aside =
    s.role === "captain" && workers > 0
      ? `captain of ${workers === 1 ? "1 worker" : `${workers} workers`}`
      : s.status === "needsFeedback" && s.why && s.why !== reason
        ? s.why
        : null;
  return (
    <div className="card-state">
      <span className={`state-pill ${TONE_TINT[l.tone]}`}>
        <span aria-hidden="true" className={`sdot ${l.tone}`} />
        {l.label} · {age(s.statusChangedAt, now)}
      </span>
      {aside && <span className={s.status === "needsFeedback" ? "card-aside-text m" : "card-aside-text"}>{aside}</span>}
      {pr && <span className={`tag tint-${pr.tint}`}>{pr.text}</span>}
    </div>
  );
}

function IssueState({ item, now }: { item: FocusItem; now: number }) {
  const i = item.issue!;
  return (
    <div className="card-state">
      {i.priority >= 1 && i.priority <= 2 && (
        <span className={i.priority === 1 ? "state-pill tint-peach" : "state-pill tint-plain"}>{priorityLabel(i.priority)}</span>
      )}
      <span className="card-aside-text">
        {i.state} ·{" "}
        {i.started ? (
          <span title={handedLine(i.started, now)}>
            {i.started.held
              ? `handed to ${i.started.projectName}, brief held`
              : `handed to ${i.started.projectName} ${ago(i.started.at, now)}`}
          </span>
        ) : (
          "no session yet"
        )}
      </span>
    </div>
  );
}

/// A captain's workers, as its project's other sessions at work or done.
function captainWorkers(captain: Session, state: State): Session[] {
  const byId = new Map(state.sessions.map((s) => [s.id, s]));
  return sessionOrder(state)
    .map((id) => byId.get(id)!)
    .filter(
      (w) =>
        w.projectId === captain.projectId &&
        w.role === "worker" &&
        (w.status === "running" || w.status === "finished" || w.status === "needsFeedback"),
    );
}

function Workers({ captain, state, now }: { captain: Session; state: State; now: number }) {
  const workers = captainWorkers(captain, state).slice(0, 3);
  if (workers.length === 0) return null;
  return (
    <ul className="card-well" aria-label={`Workers of ${shownName(captain)}`}>
      {workers.map((w) => {
        const l = look(w);
        const pr = prPill(w);
        return (
          <li key={w.id}>
            <span aria-hidden="true" className={l.pulse ? `sdot ${l.tone} pulse` : `sdot ${l.tone}`} />
            <span className="m clip">{w.name}</span>
            <span className="muted">
              {/* A finish is its grey dot, so its word gives the name its room. */}
              {l.label === "Finished" ? "" : `${l.label.toLowerCase()} `}
              {age(w.statusChangedAt, now)}
            </span>
            {pr && <span className={`tag tint-${pr.tint}`}>{pr.text}</span>}
          </li>
        );
      })}
    </ul>
  );
}

/// Where Start would hand an issue the orchestrator picked.
function StartsIn({ item, state }: { item: FocusItem; state: State }) {
  const offer = startOffer(state.projects, item.issue!);
  const to = state.projects.find((p) => p.id === offer.pick);
  if (!to) return null;
  return (
    <div className="card-well one">
      <Icon name="folder" size={14} />
      <span>
        Starts a worker in <strong>{to.name}</strong>
      </span>
    </div>
  );
}

/// What a card is answered with: the answers its session's reply offers,
/// the first drawn as the one to press, or, on an issue no session works
/// on, Start and Not now.
export function CardActs({
  item,
  s,
  busy,
  onAnswer,
  onStart,
  onNotNow,
}: {
  item: FocusItem;
  s: Session | undefined;
  busy: boolean;
  onAnswer: (item: FocusItem, n: number) => void;
  onStart: (key: string) => void;
  onNotNow: (() => void) | null;
}) {
  if (!s) {
    if (!item.issue || item.issue.started) return null;
    return (
      <div className="card-acts" role="group" aria-label={`Act on ${item.key}`}>
        <button type="button" className="answer first" onClick={() => onStart(item.key)}>
          <Icon name="play" size={13} className="filled" />
          Start
        </button>
        {onNotNow && (
          <button type="button" className="answer" onClick={onNotNow}>
            Not now
          </button>
        )}
      </div>
    );
  }
  const answers = cardAnswers(item, s);
  if (answers.length === 0) return null;
  return (
    <div className="card-acts" role="group" aria-label={`Answers for ${s.name}`}>
      {answers.map((a, i) => (
        <button
          key={a}
          type="button"
          className={i === 0 ? "answer first" : "answer"}
          disabled={busy}
          onClick={() => onAnswer(item, i + 1)}
        >
          <span className="answer-n" aria-hidden="true">
            {i + 1}
          </span>
          {a}
        </button>
      ))}
    </div>
  );
}

/// The dialog a card's session shows, its question and its options, read as
/// the session's page reads it, and picked as it picks one.
function DialogActs({ s, busy, onOption }: { s: Session; busy: boolean; onOption: (id: string, n: number) => void }) {
  const detail = useDetail(s.id, `${s.status}:${s.statusChangedAt}:${s.unseen}:${s.why}`, s.statusChangedAt);
  const dialog = detail?.dialog;
  if (!dialog) return null;
  return (
    <div className="card-acts dialog-acts" role="group" aria-labelledby={`dialog-${s.id}`}>
      <span id={`dialog-${s.id}`} className="dialog-q">
        {dialog.question}
      </span>
      {dialog.options.map((o, i) => (
        <button
          key={o.number}
          type="button"
          className={i === 0 ? "answer first" : "answer"}
          disabled={busy}
          onClick={() => onOption(s.id, o.number)}
        >
          <span className="answer-n" aria-hidden="true">
            {o.number}
          </span>
          {o.label}
        </button>
      ))}
    </div>
  );
}

/// The workers that finished today, each with its pull request's state.
export function Shipped({ state, now, onOpen }: { state: State; now: number; onOpen: (id: string) => void }) {
  const id = useId();
  const done = shippedToday(state, now);
  const prs = done.filter((s) => s.prUrl).length;
  const shown = done.slice(0, 6);
  return (
    <section className="shipped" aria-labelledby={id}>
      <div className="home-head">
        <h2 id={id}>Shipped today</h2>
        {done.length > 0 && (
          <span className="home-sub">{prs > 0 ? (prs === 1 ? "1 PR" : `${prs} PRs`) : `${done.length} done`}</span>
        )}
      </div>
      {done.length === 0 ? (
        <p className="calm">Nothing has finished today yet.</p>
      ) : (
        <div className="panel-rows">
          {shown.map((s) => {
            const pr = prPill(s);
            const meta = rowMeta({ ...s, why: null }, 0);
            return (
              <RouteLink key={s.id} to={{ kind: "session", id: s.id }} onGo={() => onOpen(s.id)} className="srow shipped-row">
                <Icon
                  name={s.prState === "merged" ? "prMerged" : s.prUrl ? "prOpen" : "done"}
                  size={15}
                  className={s.prState === "merged" ? "accent" : s.prState === "closed" ? "error" : s.prUrl ? "ok" : "muted"}
                />
                <span className="srow-text">
                  <span className="srow-top">
                    <span className="srow-title">{s.name}</span>
                    <span className="srow-age">{age(s.statusChangedAt, now)}</span>
                  </span>
                  <span className="srow-sub">
                    <span className="srow-meta" title={meta}>
                      {meta}
                    </span>
                    {pr && <span className={`tag tint-${pr.tint}`}>{pr.text}</span>}
                  </span>
                </span>
              </RouteLink>
            );
          })}
          {done.length > shown.length && <p className="list-note">and {done.length - shown.length} more</p>}
        </div>
      )}
    </section>
  );
}

/// What runs next: the nightly job's last run, whether the automation is
/// on, and the schedules soonest due.
export function ComingUp({ state, now, onGo }: { state: State; now: number; onGo: (to: Route) => void }) {
  const t = state.today;
  const nightly = t && nightlyChip(t, now);
  const failure = t && nightlyFailure(t, now);
  const next = state.schedules.available ? comingUp(state.schedules) : [];
  if (!t && next.length === 0) return null;
  return (
    <section className="coming" aria-label="Coming up">
      <div className="coming-row">
        {nightly && (
          <span className="chip">
            <span aria-hidden="true" className={nightly.ok ? "sdot green" : "sdot red"} />
            {nightly.text}
          </span>
        )}
        {t && (
          <span className="chip">
            <span aria-hidden="true" className={t.off ? "sdot peach" : "sdot green"} />
            {t.off ? "Automation off" : "Automation on"}
          </span>
        )}
        {next.length > 0 && (
          <span className="coming-next">
            <span className="coming-word">
              <Icon name="clock" size={14} className="accent" />
              Coming up
            </span>
            {next.map((s) => (
              <RouteLink
                key={s.id}
                to={{ kind: "schedule", id: s.id }}
                onGo={() => onGo({ kind: "schedule", id: s.id })}
                className="chip next"
              >
                <span className="next-name">{s.name}</span>
                <span className="muted">{dayTime(s.nextAt!, now)}</span>
              </RouteLink>
            ))}
          </span>
        )}
      </div>
      {failure && <p className="error">{failure}</p>}
    </section>
  );
}

/// The reason a pick is put aside with, on a row of its own: optional, for
/// the next rank to read.
function PickAsk({
  item,
  verdict,
  onSay,
  onClose,
}: {
  item: FocusItem;
  verdict: Verdict;
  onSay: SayOf;
  onClose: () => void;
}) {
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const field = useRef<HTMLInputElement>(null);
  useEffect(() => field.current?.focus(), []);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (busy) return;
    setBusy(true);
    const err = await onSay(item, verdict, reason.trim() || null);
    setBusy(false);
    setError(err);
  };
  return (
    <form
      className="pick-ask"
      onSubmit={submit}
      onKeyDown={(e) => {
        if (e.key === "Escape" && !composing(e.nativeEvent)) {
          e.preventDefault();
          e.stopPropagation();
          onClose();
        }
      }}
    >
      <label htmlFor={`say-${item.key}`}>{verdict === "notToday" ? "Why not today?" : "Why is it wrong?"}</label>
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
          {verdict === "notToday" ? "Not today" : "Wrong"}
        </button>
        <button type="button" className="btn" onClick={onClose}>
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

/// Not today and Wrong beside a pick's why on its page: each asks for an
/// optional reason on a row of its own before it puts the pick aside.
export function PickFeedback({ item, onSay }: { item: FocusItem; onSay: SayOf }) {
  const [asking, setAsking] = useState<Verdict | null>(null);
  if (asking) return <PickAsk item={item} verdict={asking} onSay={onSay} onClose={() => setAsking(null)} />;
  return (
    <span className="pick-say">
      <button type="button" className="quiet-btn" onClick={() => setAsking("notToday")}>
        Not today
      </button>
      <button type="button" className="quiet-btn" onClick={() => setAsking("wrong")}>
        Wrong
      </button>
    </span>
  );
}
