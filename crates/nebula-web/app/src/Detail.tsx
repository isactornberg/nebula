import { useMemo, useState, type KeyboardEvent } from "react";
import { useScreen } from "./api/client";
import type { Focus, Issue, Session, SessionDetail, State } from "./api/contract";
import {
  age,
  ago,
  answersShown,
  canSend,
  issueSite,
  prLabel,
  prStateLabel,
  priorityLabel,
  sessionIssue,
  splitReply,
  stateTone,
  statusLook,
  when,
  type Flash,
} from "./model";
import { Priority } from "./Side";
import { Dot, External, FlashSpot, Markdown, md, RouteLink } from "./ui";

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

/// An assigned issue no session works on: what Linear says of it, and a
/// link to it there.
export function IssueDetail({ issue: i, now, onBack }: { issue: Issue; now: number; onBack: () => void }) {
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
          <External className="btn" href={i.url}>
            Linear ↗
          </External>
        </div>
      </div>

      <div className="title-block">
        <h1>{i.title}</h1>
        <p className="why muted">Assigned to you, and no session works on it.</p>
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
