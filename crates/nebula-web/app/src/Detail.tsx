import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type RefObject } from "react";
import { useScreen } from "./api/client";
import type { Focus, Issue, Project, Session, SessionDetail, State } from "./api/contract";
import {
  age,
  ago,
  answersShown,
  canSend,
  handedLine,
  issueSite,
  prLabel,
  prStateLabel,
  priorityLabel,
  sessionIssue,
  splitReply,
  startOffer,
  stateTone,
  statusLook,
  when,
  type Flash,
} from "./model";
import { Priority } from "./Side";
import { arrowsMove, Dot, External, FlashSpot, Markdown, md, RouteLink } from "./ui";

type Tab = "reply" | "workers" | "screen";

export function Detail({
  state,
  session: s,
  detail,
  pick,
  focus,
  now,
  busy,
  error,
  canShow,
  shown,
  onBack,
  onTerminal,
  onPick,
  onSend,
  onOpen,
}: {
  state: State;
  session: Session;
  detail: SessionDetail | null;
  pick: { n: number; reason: string } | null;
  focus: Focus;
  now: number;
  busy: boolean;
  error: string | null;
  /// A TUI is open to put the session up.
  canShow: boolean;
  /// What Open in terminal said, for a moment.
  shown: (Flash & { seq: number }) | null;
  onBack: () => void;
  onTerminal: () => void;
  onPick: (n: number) => void;
  onSend: (text: string) => Promise<boolean>;
  onOpen: (id: string) => void;
}) {
  const [tab, setTab] = useState<Tab>("reply");
  const look = statusLook(s);
  const linked = sessionIssue(s);
  const answers = detail ? answersShown(detail) : [];
  const tabs: [Tab, string][] = [
    ["reply", "Reply"],
    ...(s.role === "captain" ? [["workers", `Workers ${detail?.workerIds.length ?? ""}`.trim()] as [Tab, string]] : []),
    ["screen", "Screen"],
  ];

  return (
    <main className="pane detail">
      <div className="detail-top">
        <RouteLink to={{ kind: "idle" }} onGo={onBack} className="back">
          ‹ Today
        </RouteLink>
        <span className="meta">
          {linked && (
            <>
              <External href={linked.url}>{linked.key}</External>
              <span className="sep"> · </span>
            </>
          )}
          <Dot tone={look.tone} /> {look.label}
          <span className="sep"> · </span>
          {s.projectName}
        </span>
        <div className="actions">
          <span className="flash-anchor">
            {canShow && (
              <button type="button" className="btn" onClick={onTerminal} aria-keyshortcuts="t">
                Open in terminal <kbd aria-hidden="true">t</kbd>
              </button>
            )}
            <FlashSpot flash={shown} />
          </span>
          {linked && (
            <External className="btn" href={linked.url}>
              {issueSite(linked.url)} ↗
            </External>
          )}
          {s.prUrl && (
            <External className="btn" href={s.prUrl}>
              {prLabel(s.prUrl)} ↗
            </External>
          )}
        </div>
      </div>

      <div className="title-block">
        <h1>{s.name}</h1>
        {pick ? (
          <p className="why">
            <strong className="accent">Why focus #{pick.n}</strong>
            {focus.source === "rule" && <span className="muted"> by rule</span>}
            {pick.reason && ` · ${pick.reason}`}
          </p>
        ) : (
          s.status === "needsFeedback" && s.why && <p className="why">{s.why}</p>
        )}
      </div>

      <div className="tabs" role="tablist" aria-label="Detail">
        {tabs.map(([id, label]) => (
          <button
            key={id}
            type="button"
            role="tab"
            id={`tab-${id}`}
            aria-selected={tab === id}
            aria-controls="detail-panel"
            onClick={() => setTab(id)}
          >
            {label}
          </button>
        ))}
      </div>

      <div id="detail-panel" role="tabpanel" aria-labelledby={`tab-${tab}`}>
        {tab === "reply" && <ReplyPanel detail={detail} now={now} />}
        {tab === "workers" && (
          <WorkersPanel state={state} ids={detail?.workerIds ?? []} now={now} onOpen={onOpen} />
        )}
        {tab === "screen" && <ScreenPanel id={s.id} />}
      </div>

      {detail?.dialog && (
        <section className="dialog" aria-labelledby="dialog-question">
          <p id="dialog-question">{detail.dialog.question}</p>
          <div className="options">
            {detail.dialog.options.map((o) => (
              <button
                key={o.number}
                type="button"
                className="option"
                disabled={busy}
                onClick={() => onPick(o.number)}
              >
                <span className="badge">{o.number}</span>
                {o.label}
              </button>
            ))}
          </div>
        </section>
      )}

      {answers.length > 0 && (
        <div className="pills" role="group" aria-label="Answers">
          {answers.map((a, i) => (
            <button
              key={a}
              type="button"
              className={i === 0 ? "pill first" : "pill"}
              disabled={busy}
              onClick={() => onPick(i + 1)}
            >
              {i + 1} {a}
            </button>
          ))}
        </div>
      )}

      <Composer session={s} dialogOpen={!!detail?.dialog} answered={answers.length > 0} busy={busy} onSend={onSend} />
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}

      <section className="linked" aria-labelledby="linked-title">
        <h2 id="linked-title" className="caps muted">
          Linked
        </h2>
        <dl>
          <div>
            <dt>Issue</dt>
            <dd>
              {linked ? (
                <>
                  <External href={linked.url}>{linked.key} ↗</External>
                  {s.issue && (
                    <>
                      <span className="clip" title={s.issue.title}>
                        {s.issue.title}
                      </span>
                      <span className="muted">· {s.issue.state}</span>
                    </>
                  )}
                </>
              ) : s.issueUrl ? (
                <External href={s.issueUrl}>{issueSite(s.issueUrl)} ↗</External>
              ) : (
                <span className="muted">none</span>
              )}
            </dd>
          </div>
          <div>
            <dt>Branch</dt>
            <dd>
              <span className="clip">{s.branch}</span>
              {s.rootCheckout && <span className="muted">· root checkout</span>}
            </dd>
          </div>
          <div>
            <dt>PR</dt>
            <dd>
              {s.prUrl ? (
                <>
                  <External href={s.prUrl}>{prLabel(s.prUrl)} ↗</External>
                  {s.prState && <span className={`pr-state pr-${s.prState}`}>· {prStateLabel(s.prState)}</span>}
                </>
              ) : (
                <span className="muted">none yet</span>
              )}
            </dd>
          </div>
        </dl>
      </section>
    </main>
  );
}

/// An assigned issue no session works on: what Linear says of it, a link
/// to it there, and Start, which asks on a row of its own first.
export function IssueDetail({
  issue: i,
  state,
  now,
  asking,
  busy,
  error,
  onBack,
  onAsk,
  onStart,
}: {
  issue: Issue;
  state: State;
  now: number;
  /// The Start row is open.
  asking: boolean;
  busy: boolean;
  error: string | null;
  onBack: () => void;
  onAsk: (open: boolean) => void;
  onStart: (project: string, again: boolean) => void;
}) {
  const offer = startOffer(state.projects, i);
  const startButton = useRef<HTMLButtonElement>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);
  const wasAsking = useRef(false);
  // The keys go to Cancel while it asks, and back to Start after.
  useEffect(() => {
    if (asking) cancelButton.current?.focus();
    else if (wasAsking.current) startButton.current?.focus();
    wasAsking.current = asking;
  }, [asking]);
  return (
    <main className="pane detail">
      <div className="detail-top">
        <RouteLink to={{ kind: "idle" }} onGo={onBack} className="back">
          ‹ Today
        </RouteLink>
        <span className="meta">
          <External href={i.url}>{i.key}</External>
          <span className="sep"> · </span>
          <Dot tone={stateTone(i.stateType)} /> {i.state}
          {i.project && (
            <>
              <span className="sep"> · </span>
              {i.project}
            </>
          )}
        </span>
        <div className="actions">
          {offer.pick && !asking && (
            <button
              ref={startButton}
              type="button"
              className="btn"
              aria-keyshortcuts="s"
              title="Hands the issue to a project's captain, which starts a worker on it."
              onClick={() => onAsk(true)}
            >
              Start <kbd aria-hidden="true">s</kbd>
            </button>
          )}
          <External className="btn" href={i.url}>
            Linear ↗
          </External>
        </div>
      </div>

      {asking && offer.pick && (
        <StartAsk
          key={i.key}
          issue={i}
          state={state}
          offered={offer.projects}
          first={offer.pick}
          busy={busy}
          error={error}
          cancelRef={cancelButton}
          onCancel={() => onAsk(false)}
          onStart={onStart}
        />
      )}

      <div className="title-block">
        <h1>{i.title}</h1>
        {i.started ? (
          <p className="why handed">{handedLine(i.started, now)}</p>
        ) : (
          <p className="why muted">Assigned to you, and no session works on it.</p>
        )}
        {i.started?.moveFailed && (
          <p className="why move-failed">It was not moved to its started state in Linear: {i.started.moveFailed}</p>
        )}
        {!offer.pick && <p className="why muted">No project reads its issues from Linear's {i.team} team, so Start has nowhere to hand it.</p>}
      </div>

      <section className="linked" aria-labelledby="issue-title">
        <h2 id="issue-title" className="caps muted">
          In Linear
        </h2>
        <dl>
          <div>
            <dt>Key</dt>
            <dd>
              <External href={i.url}>{i.key} ↗</External>
            </dd>
          </div>
          <div>
            <dt>State</dt>
            <dd>
              <span>
                <Dot tone={stateTone(i.stateType)} /> {i.state}
              </span>
            </dd>
          </div>
          <div>
            <dt>Priority</dt>
            <dd className="with-mark">
              <Priority p={i.priority} />
              {priorityLabel(i.priority)}
            </dd>
          </div>
          <div>
            <dt>Project</dt>
            <dd>{i.project ? <span className="clip">{i.project}</span> : <span className="muted">none</span>}</dd>
          </div>
          <div>
            <dt>Updated</dt>
            <dd>
              {i.updatedAt > 0 ? (
                <>
                  {when(i.updatedAt, now)}
                  <span className="muted">· {ago(i.updatedAt, now)}</span>
                </>
              ) : (
                <span className="muted">unknown</span>
              )}
            </dd>
          </div>
          {i.branchName && (
            <div>
              <dt>Branch</dt>
              <dd>
                <span className="clip">{i.branchName}</span>
              </dd>
            </div>
          )}
        </dl>
      </section>
    </main>
  );
}

/// What handing an issue to `project`'s captain does, by how its captain
/// stands now.
function handOff(state: State, project: Project): string {
  const captain = state.sessions.find((s) => s.role === "captain" && s.projectId === project.id);
  const worker = "starts a worker on the issue, on a branch of its own";
  if (!captain) return `${project.name} has no captain yet, so nebula starts one, which ${worker}.`;
  switch (captain.status) {
    case "running":
    case "needsFeedback":
      return `Its captain ${worker}, once it is done with what it is at: the brief waits until then.`;
    case "terminated":
    case "disconnected":
      return `Its captain is not running, so nebula brings it back, and once it is up it ${worker}.`;
    default:
      return `Its captain is at rest, so it takes the brief now and ${worker}.`;
  }
}

/// Start's question, under the top of an issue's page: the project it goes
/// to, what the hand-off does there and in Linear, and its own Start. Only
/// that Start hands it over; Cancel and Esc leave.
function StartAsk({
  issue: i,
  state,
  offered,
  first,
  busy,
  error,
  cancelRef,
  onCancel,
  onStart,
}: {
  issue: Issue;
  state: State;
  offered: Project[];
  first: string;
  busy: boolean;
  error: string | null;
  cancelRef: RefObject<HTMLButtonElement | null>;
  onCancel: () => void;
  onStart: (project: string, again: boolean) => void;
}) {
  const [picked, setPicked] = useState(first);
  const project = offered.find((p) => p.id === picked) ?? offered[0]!;
  // While its Start is under way the row says what it asked: the state the
  // hand-off changes as it goes is not its answer.
  const asked = useRef({ state, issue: i });
  if (!busy) asked.current = { state, issue: i };
  const { state: was, issue: then } = asked.current;
  const again = !!then.started;
  return (
    <div className="fix-confirm start-confirm" role="group" aria-labelledby="start-ask">
      <p id="start-ask">
        {again
          ? `${i.key} was handed to the captain of ${then.started!.projectName} already. Start it again in ${project.name}?`
          : `Start ${i.key} in ${project.name}?`}
      </p>
      {offered.length > 1 && (
        <div className="start-pick">
          <span className="quiet" id="start-projects">
            Project
          </span>
          <div role="group" aria-labelledby="start-projects" className="positions" onKeyDown={arrowsMove}>
            {offered.map((p) => (
              <button key={p.id} type="button" aria-pressed={p.id === project.id} onClick={() => setPicked(p.id)}>
                {p.name}
              </button>
            ))}
          </div>
        </div>
      )}
      <p className="quiet">{handOff(was, project)}</p>
      <p className="quiet">
        {then.stateType === "started"
          ? `It is ${then.state} in Linear already, and stays there.`
          : `In Linear the issue moves from ${then.state} to its team's first started state.`}
      </p>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <div className="confirm-acts">
        <button
          type="button"
          className="btn primary"
          disabled={busy}
          aria-busy={busy || undefined}
          onClick={() => onStart(project.id, again)}
        >
          {busy ? "Starting…" : again ? `Start ${i.key} again` : `Start ${i.key}`}
        </button>
        <button ref={cancelRef} type="button" className="fix-act" onClick={onCancel}>
          Cancel
        </button>
        <span className="quiet">The worker shows on the board once the captain starts it.</span>
      </div>
    </div>
  );
}

function ReplyPanel({ detail, now }: { detail: SessionDetail | null; now: number }) {
  const reply = detail?.reply;
  const split = useMemo(
    () => (reply ? splitReply(reply.text, (detail?.answers.length ?? 0) > 0) : null),
    [reply, detail?.answers.length],
  );
  if (!detail) return <p className="muted">Reading the reply…</p>;
  if (!reply || !split) return <p className="muted">No reply yet.</p>;
  const { body, report } = split;
  return (
    <div className="reply">
      <div className="reply-meta">
        {reply.source === "transcript" ? "Last reply" : "Last reply, read off the screen"}
        {reply.at !== null && ` · ${when(reply.at, now)}`}
      </div>
      {body &&
        (reply.source === "transcript" ? (
          <Markdown text={body} />
        ) : (
          <pre className="plain">{body}</pre>
        ))}
      {report && (
        <p className="report">
          <span className={report.word === "DONE" ? "report-word tone-blue" : "report-word tone-red"}>
            {report.word}
          </span>{" "}
          <span dangerouslySetInnerHTML={{ __html: md.renderInline(report.text) }} />
        </p>
      )}
    </div>
  );
}

function WorkersPanel({
  state,
  ids,
  now,
  onOpen,
}: {
  state: State;
  ids: string[];
  now: number;
  onOpen: (id: string) => void;
}) {
  const workers = ids.flatMap((id) => state.sessions.filter((s) => s.id === id));
  if (workers.length === 0) return <p className="muted">No workers.</p>;
  return (
    <ul className="workers">
      {workers.map((w) => {
        const look = statusLook(w);
        return (
          <li key={w.id}>
            <RouteLink to={{ kind: "session", id: w.id }} onGo={() => onOpen(w.id)} className="worker">
              <Dot tone={look.tone} />
              <span className="w-name">{w.name}</span>
              <span className="w-why">{w.why ?? ""}</span>
              <span className="w-state">
                {look.label} · {age(w.statusChangedAt, now)}
              </span>
            </RouteLink>
          </li>
        );
      })}
    </ul>
  );
}

function ScreenPanel({ id }: { id: string }) {
  const screen = useScreen(id);
  if (!screen) return <p className="muted">Reading the screen…</p>;
  return (
    <pre className="screen" aria-label="Terminal screen" tabIndex={0}>
      {screen.rows.join("\n").trimEnd()}
    </pre>
  );
}

function Composer({
  session,
  dialogOpen,
  answered,
  busy,
  onSend,
}: {
  session: Session;
  dialogOpen: boolean;
  answered: boolean;
  busy: boolean;
  onSend: (text: string) => Promise<boolean>;
}) {
  const [text, setText] = useState("");
  const blocked = dialogOpen
    ? "A dialog is open: answer it first."
    : session.status === "running"
      ? "Running: send once the turn is done."
      : !canSend(session)
        ? "The session has stopped and takes no follow-up."
        : null;
  const submit = async () => {
    const t = text.trim();
    if (!t || blocked || busy) return;
    if (await onSend(t)) setText("");
  };
  const enter = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key !== "Enter" || e.shiftKey || e.nativeEvent.isComposing) return;
    e.preventDefault();
    void submit();
  };
  return (
    <form
      className="composer"
      onSubmit={(e) => {
        e.preventDefault();
        void submit();
      }}
    >
      <label htmlFor="reply-text" className="muted">
        {answered ? "Or answer in your own words" : "Send a follow-up"}
      </label>
      <textarea
        id="reply-text"
        rows={2}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={enter}
        placeholder="Enter sends, Shift+Enter adds a line"
      />
      <div className="composer-foot">
        <button type="submit" className="btn" disabled={!!blocked || busy || !text.trim()}>
          Send <span aria-hidden="true">⏎</span>
        </button>
        <span className="quiet" role="status">
          {busy ? "Sending…" : (blocked ?? "Sent as a follow-up, the same as the TUI.")}
        </span>
      </div>
    </form>
  );
}
