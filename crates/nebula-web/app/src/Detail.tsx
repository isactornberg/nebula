import MarkdownIt from "markdown-it";
import { useMemo, useState, type KeyboardEvent } from "react";
import { useScreen } from "./api/client";
import type { Focus, Session, SessionDetail, State } from "./api/contract";
import {
  age,
  answersShown,
  canSend,
  issueSite,
  prLabel,
  splitReply,
  statusLook,
  when,
} from "./model";
import { Dot, External, RouteLink } from "./ui";

// Raw HTML in a reply is shown as text, never parsed (`html: false`), and
// markdown-it drops javascript: and data: links.
const md = new MarkdownIt({ html: false, linkify: true });
const linkOpen = md.renderer.rules.link_open ?? ((t, i, o, _e, self) => self.renderToken(t, i, o));
md.renderer.rules.link_open = (tokens, idx, options, env, self) => {
  tokens[idx]!.attrSet("target", "_blank");
  tokens[idx]!.attrSet("rel", "noreferrer");
  return linkOpen(tokens, idx, options, env, self);
};

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
  onBack: () => void;
  onTerminal: () => void;
  onPick: (n: number) => void;
  onSend: (text: string) => Promise<boolean>;
  onOpen: (id: string) => void;
}) {
  const [tab, setTab] = useState<Tab>("reply");
  const look = statusLook(s);
  const key = s.issueKey;
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
          {key && s.issueUrl && (
            <>
              <External href={s.issueUrl}>{key}</External>
              <span className="sep"> · </span>
            </>
          )}
          <Dot tone={look.tone} /> {look.label}
          <span className="sep"> · </span>
          {s.projectName}
        </span>
        <div className="actions">
          {canShow && (
            <button type="button" className="btn" onClick={onTerminal} aria-keyshortcuts="t">
              Open in terminal <kbd>t</kbd>
            </button>
          )}
          {s.issueUrl && (
            <External className="btn" href={s.issueUrl}>
              {issueSite(s.issueUrl)} ↗
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
              {s.issueUrl ? (
                <External href={s.issueUrl}>{key ?? issueSite(s.issueUrl)} ↗</External>
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
                <External href={s.prUrl}>{prLabel(s.prUrl)} ↗</External>
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

function Markdown({ text }: { text: string }) {
  const html = useMemo(() => md.render(text), [text]);
  return <div className="md" dangerouslySetInnerHTML={{ __html: html }} />;
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
          Send ⏎
        </button>
        <span className="quiet" role="status">
          {busy ? "Sending…" : (blocked ?? "Sent as a follow-up, the same as the TUI.")}
        </span>
      </div>
    </form>
  );
}
