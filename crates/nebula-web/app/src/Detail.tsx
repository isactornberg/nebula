import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type MouseEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { useScreen } from "./api/client";
import type { Focus, Issue, Project, Session, SessionDetail, State, Turns } from "./api/contract";
import { ComposeForm, useCompose } from "./Compose";
import { PickFeedback, type SayOf } from "./Idle";
import {
  age,
  ago,
  answersShown,
  canSend,
  captainOf,
  handedLine,
  harnessName,
  issueSite,
  look,
  prLabel,
  prPill,
  prStateLabel,
  priorityLabel,
  sessionIssue,
  startOffer,
  TONE_TINT,
  when,
  type Flash,
  type FocusItem,
} from "./model";
import { Priority } from "./Side";
import { Thread, threadItems, useStick } from "./Thread";
import { arrowsMove, External, FlashSpot, Icon, RouteLink, type IconName } from "./ui";

/// How much room the page has: the list beside the open pane, the pane
/// alone, or a phone.
export type Layout = "wide" | "single" | "phone";

/// A session's page: its title and state, the issue it works on and the
/// pull requests it opened, its conversation, and under it what answers
/// it; beside the thread, or in its place, its live terminal.
export function Detail({
  state,
  session: s,
  detail,
  turns,
  pick,
  note,
  onSay,
  focus,
  now,
  busy,
  error,
  layout,
  back,
  terminal,
  canShow,
  shown,
  follow,
  banner,
  onBack,
  onTerminal,
  onShow,
  onFollow,
  onSeen,
  onPick,
  onSend,
  onOpen,
}: {
  state: State;
  session: Session;
  detail: SessionDetail | null;
  /// Its conversation turn by turn, once read.
  turns: Turns | null;
  pick: FocusItem | null;
  /// What the page said of the pick put aside last.
  note: string | null;
  /// Not today and Wrong, on the orchestrator's picks alone.
  onSay: SayOf | null;
  focus: Focus;
  now: number;
  busy: boolean;
  error: string | null;
  layout: Layout;
  /// The name of the list Back goes to.
  back: string;
  /// The live terminal is shown.
  terminal: boolean;
  /// A TUI is open to put the session up.
  canShow: boolean;
  /// What Show in TUI said, for a moment.
  shown: (Flash & { seq: number }) | null;
  /// Whether the page follows the TUI, null with no TUI open.
  follow: boolean | null;
  /// What the page says across its top.
  banner: ReactNode;
  onBack: () => void;
  onTerminal: () => void;
  onShow: () => void;
  onFollow: () => void;
  onSeen: () => void;
  onPick: (n: number) => void;
  onSend: (text: string, images: string[]) => Promise<boolean>;
  onOpen: (id: string) => void;
}) {
  const answers = detail ? answersShown(detail) : [];
  const ids = detail?.workerIds;
  const workers = useMemo(
    () => (ids ?? []).flatMap((id) => state.sessions.filter((w) => w.id === id)),
    [ids, state.sessions],
  );
  const items = useMemo(
    () => threadItems(s, detail, turns, s.role === "captain" ? workers : []),
    [s, detail, turns, workers],
  );
  const split = terminal && layout === "wide";
  useKeyboardFit(layout === "phone");
  const live = terminal && <Terminal s={s} canShow={canShow} shown={shown} onShow={onShow} onHide={onTerminal} />;
  const menu = (
    <MoreMenu phone={layout === "phone"}>
      {layout === "phone" && canShow && (
        <MenuItem icon="monitor" onClick={onShow}>
          Show in TUI
        </MenuItem>
      )}
      {layout === "phone" && s.unseen && (
        <MenuItem icon="eye" onClick={onSeen}>
          Mark seen
        </MenuItem>
      )}
      {follow !== null && (
        <MenuItem icon="terminal" onClick={onFollow} pressed={follow}>
          {follow ? "Following TUI" : "Follow TUI"}
        </MenuItem>
      )}
      <SessionLinks s={s} />
      <p className="menu-note">
        <span className="m">{s.branch}</span>
        {s.rootCheckout && " · root checkout"}
      </p>
    </MoreMenu>
  );
  const termButton =
    layout === "phone" ? (
      <button
        type="button"
        className="icon-btn"
        aria-pressed={terminal}
        aria-label="Live terminal"
        aria-keyshortcuts="t"
        onClick={onTerminal}
      >
        <Icon name="terminal" size={20} />
      </button>
    ) : (
      <button
        type="button"
        className="btn"
        aria-pressed={terminal}
        aria-keyshortcuts="t"
        title={terminal ? "Hide the live terminal" : "Show the live terminal"}
        onClick={onTerminal}
      >
        <Icon name="terminal" size={15} />
        <span className="hl">Terminal</span>
        <kbd aria-hidden="true">T</kbd>
      </button>
    );
  const l = look(s);
  const key = sessionIssue(s)?.key;

  return (
    <main className="sess" data-layout={layout} data-split={split || undefined}>
      <section className="sess-main" aria-label={s.name}>
        <header className="sess-head">
          {layout === "phone" ? (
            <div className="sess-nav">
              <BackLink back={back} onBack={onBack} />
              <span className="sess-gap" />
              {termButton}
              <span className="flash-anchor">
                {menu}
                {!terminal && <FlashSpot flash={shown} />}
              </span>
            </div>
          ) : (
            layout === "single" && <BackLink back={back} onBack={onBack} />
          )}
          {banner}
          <div className="sess-top">
            <h1 className="sess-title">{s.name}</h1>
            {layout !== "phone" && s.role === "captain" && (
              <span className="cap-badge tint-mauve">
                <Icon name="anchor" size={13} />
                Captain
              </span>
            )}
            {layout !== "phone" && (
              <span className={`state-pill ${TONE_TINT[l.tone]}`}>
                {l.label} · {age(s.statusChangedAt, now)}
              </span>
            )}
            {layout !== "phone" && (
              <div className="sess-acts">
                {s.unseen && (
                  <button type="button" className="btn" title="Mark seen" onClick={onSeen}>
                    <Icon name="eye" size={15} />
                    <span className="hl">Mark seen</span>
                  </button>
                )}
                {canShow && !terminal && (
                  <span className="flash-anchor">
                    <button type="button" className="btn" title="Show in TUI" onClick={onShow}>
                      <Icon name="monitor" size={15} />
                      <span className="hl">Show in TUI</span>
                    </button>
                    <FlashSpot flash={shown} />
                  </span>
                )}
                {termButton}
                {menu}
              </div>
            )}
          </div>
          {layout === "phone" ? (
            <>
              <p className="sess-sub">
                {s.projectName}
                {key ? ` · ${key}` : !s.rootCheckout && s.branch !== s.name ? ` · ${s.branch}` : ""}
              </p>
              <LifeChips s={s} workers={workers} />
            </>
          ) : (
            <Life s={s} state={state} workers={workers} now={now} onOpen={onOpen} />
          )}
        </header>
        {terminal && !split ? (
          live
        ) : (
          <ThreadPane
            top={<WhyBanner pick={pick} rule={focus.source === "rule"} note={note} onSay={onSay} />}
            empty={
              !detail && !turns
                ? "Reading the conversation…"
                : s.status === "fresh"
                  ? "Nothing here yet: the session has not started."
                  : "No reply yet."
            }
          >
            {items.length > 0 ? <Thread items={items} s={s} now={now} dropNext={answers.length > 0} onOpen={onOpen} /> : null}
          </ThreadPane>
        )}
        <Dock s={s} detail={detail} answers={answers} phone={layout === "phone"} busy={busy} error={error} onPick={onPick} onSend={onSend} />
      </section>
      {split && live}
    </main>
  );
}

export function BackLink({ back, onBack }: { back: string; onBack: () => void }) {
  return (
    <RouteLink to={{ kind: "idle" }} onGo={onBack} className="sess-back">
      <Icon name="chevronLeft" size={22} />
      {back}
    </RouteLink>
  );
}

/// The thread's scroller: at its bottom when it opens, and kept there as
/// turns come while the user has not scrolled up.
function ThreadPane({ top, empty, children }: { top: ReactNode; empty: string; children: ReactNode }) {
  const scroller = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  useStick(scroller, content);
  return (
    <div className="sess-scroll" ref={scroller}>
      <div className="sess-col" ref={content}>
        {top}
        <div className="t-gap" />
        {children ?? <p className="t-empty">{empty}</p>}
      </div>
    </div>
  );
}

/// Why a session or an issue is in focus, and, for one of the
/// orchestrator's picks, Not today and Wrong; else what the page said of
/// the pick put aside last.
function WhyBanner({
  pick,
  rule,
  note,
  onSay,
}: {
  pick: FocusItem | null;
  rule: boolean;
  note: string | null;
  onSay: SayOf | null;
}) {
  if (!pick) {
    return note ? (
      <p className="why-banner put-aside" role="status">
        {note}
      </p>
    ) : null;
  }
  return (
    <div className="why-banner">
      <Icon name="sparkle" size={16} className="accent" />
      <p className="why-text">
        <span className="why-n">Focus #{pick.n}</span>
        {rule && " by rule"}
        {pick.reason && ` · ${pick.reason}`}
      </p>
      {onSay && <PickFeedback item={pick} onSay={onSay} />}
    </div>
  );
}

/// The lifecycle across the head of a session's page: the issue it works
/// on, the session, and its pull request, or a captain's workers and theirs.
function Life({
  s,
  state,
  workers,
  now,
  onOpen,
}: {
  s: Session;
  state: State;
  workers: Session[];
  now: number;
  onOpen: (id: string) => void;
}) {
  const issue = sessionIssue(s);
  const l = look(s);
  const captain = captainOf(s, state);
  return (
    <nav className="life" aria-label="Issue, session and pull requests">
      {issue && (
        <>
          <External href={issue.url} className="life-step life-issue">
            <span className="life-head">
              Issue
              {s.issue ? (
                <>
                  <span aria-hidden="true" className={s.issue.stateType === "started" ? "sdot yellow" : "sdot hollow"} />
                  <span className="life-state">{s.issue.state}</span>
                </>
              ) : (
                <span className="life-state">on {issueSite(issue.url)}</span>
              )}
            </span>
            <span className="life-main">
              <span className="m life-key">{issue.key}</span>
              {s.issue && <span className="life-title">{s.issue.title}</span>}
            </span>
          </External>
          <Step />
        </>
      )}
      <div className="life-step life-session" aria-current="step">
        <span className="life-head">
          Session
          <span aria-hidden="true" className={`sdot ${l.tone}`} />
          <span className="life-state">
            {l.label} · {age(s.statusChangedAt, now)}
          </span>
        </span>
        <span className="life-main">
          <span className="life-name">{s.name}</span>
          <span className="life-meta">
            {s.role === "captain" ? (
              `captain · ${s.branch}`
            ) : captain ? (
              <>
                worker of{" "}
                <RouteLink to={{ kind: "session", id: captain.id }} onGo={() => onOpen(captain.id)}>
                  {captain.name}
                </RouteLink>
              </>
            ) : (
              <span className="m">{s.branch}</span>
            )}
          </span>
        </span>
      </div>
      <Step />
      {s.role === "captain" ? (
        <div className="life-step life-workers">
          <span className="life-head">
            <span>
              Workers<span className="life-and"> and PRs</span>
            </span>
          </span>
          <span className="life-main">
            {workers.length === 0 ? (
              <span className="life-none">No workers yet</span>
            ) : (
              workers.slice(0, 3).map((w) => <WorkerLink key={w.id} w={w} onOpen={onOpen} />)
            )}
            {workers.length > 3 && <span className="life-none">+{workers.length - 3}</span>}
          </span>
        </div>
      ) : s.prUrl && s.prState ? (
        <External href={s.prUrl} className="life-step life-pr">
          <span className="life-head">PR</span>
          <span className="life-main">
            <Icon name={s.prState === "merged" ? "prMerged" : "prOpen"} size={14} />
            {prLabel(s.prUrl)}
            <span className={`tag tint-${prPill(s)!.tint}`}>{prStateLabel(s.prState)}</span>
          </span>
        </External>
      ) : (
        <div className="life-step life-pr">
          <span className="life-head">PR</span>
          <span className="life-main life-none">
            <svg aria-hidden="true" className="icon" width="13" height="13" viewBox="0 0 24 24" strokeDasharray="3 3">
              <circle cx="12" cy="12" r="8" />
            </svg>
            No PR yet
          </span>
        </div>
      )}
    </nav>
  );
}

function Step() {
  return (
    <span aria-hidden="true" className="life-to">
      <Icon name="chevronRight" size={14} />
    </span>
  );
}

const WORKER_ICON: Record<Session["status"], IconName> = {
  running: "running",
  finished: "check",
  needsFeedback: "shield",
  fresh: "play",
  terminated: "close",
  disconnected: "close",
};

/// The colour of a worker's mark: at work or done, a dialog, a question.
function workerTone(w: Session): string {
  if (w.reportWord) return "error";
  if (w.status === "needsFeedback") return "late";
  return w.status === "running" || w.status === "finished" ? "ok" : "muted";
}

function WorkerLink({ w, onOpen }: { w: Session; onOpen: (id: string) => void }) {
  const l = look(w);
  const pr = prPill(w);
  return (
    <RouteLink to={{ kind: "session", id: w.id }} onGo={() => onOpen(w.id)} className="life-worker">
      <Icon name={WORKER_ICON[w.status]} size={13} className={workerTone(w)} />
      <span className="m">{w.name}</span>
      {pr ? <span className={`tag tint-${pr.tint}`}>{pr.text}</span> : <span className="life-none">{l.label.toLowerCase()}</span>}
    </RouteLink>
  );
}

/// The lifecycle on a phone: a row of chips.
function LifeChips({ s, workers }: { s: Session; workers: Session[] }) {
  const issue = sessionIssue(s);
  const l = look(s);
  const captain = s.role === "captain";
  const prs = (captain ? workers : [s]).flatMap((w) => {
    const pill = prPill(w);
    return pill && w.prUrl ? [{ id: w.id, url: w.prUrl, pill }] : [];
  });
  return (
    <div className="life-chips" role="group" aria-label="Issue, session and pull requests">
      {issue && (
        <External href={issue.url} className="lchip">
          <span aria-hidden="true" className={s.issue?.stateType === "started" ? "sdot yellow" : "sdot hollow"} />
          <span className="m">{issue.key}</span>
        </External>
      )}
      <span className="lchip-pair">
        {issue && <ChipStep />}
        <span className={`lchip ${captain ? "tint-mauve" : TONE_TINT[l.tone]}`}>{captain ? "Captain" : l.label}</span>
      </span>
      {captain && workers.length > 0 && (
        <span className="lchip-pair">
          <ChipStep />
          <span className="lchip">{workers.length === 1 ? "1 worker" : `${workers.length} workers`}</span>
        </span>
      )}
      {prs.slice(0, 2).map((p) => (
        <span key={p.id} className="lchip-pair">
          <ChipStep />
          <External href={p.url} className={`lchip tint-${p.pill.tint}`} title={p.pill.text}>
            {prLabel(p.url)}
          </External>
        </span>
      ))}
    </div>
  );
}

function ChipStep() {
  return <Icon name="chevronRight" size={10} className="lchip-to" />;
}

/// The links the ⋯ menu offers: the issue, and the pull request.
function SessionLinks({ s }: { s: Session }) {
  const issue = sessionIssue(s);
  return (
    <>
      {issue && (
        <MenuLink icon="issues" href={issue.url}>
          Open {issue.key} in {issueSite(issue.url)}
        </MenuLink>
      )}
      {!issue && s.issueUrl && (
        <MenuLink icon="issues" href={s.issueUrl}>
          Open the issue in {issueSite(s.issueUrl)}
        </MenuLink>
      )}
      {s.prUrl && (
        <MenuLink icon={s.prState === "merged" ? "prMerged" : "prOpen"} href={s.prUrl}>
          Open {prLabel(s.prUrl)} on GitHub
        </MenuLink>
      )}
    </>
  );
}

function hideMenu(e: MouseEvent<HTMLElement>) {
  (e.currentTarget.closest("[popover]") as HTMLElement | null)?.hidePopover?.();
}

/// ⋯ and the menu it opens: what the head has no room for.
function MoreMenu({ phone, children }: { phone: boolean; children: ReactNode }) {
  const id = `more${useId().replace(/[^\w-]/g, "")}`;
  return (
    <>
      <button type="button" className={phone ? "icon-btn more-btn" : "btn sq more-btn"} aria-label="More actions" popoverTarget={id}>
        <Icon name="more" size={phone ? 20 : 16} />
      </button>
      <div id={id} popover="auto" className="menu">
        {children}
      </div>
    </>
  );
}

function MenuItem({
  icon,
  pressed,
  onClick,
  children,
}: {
  icon: IconName;
  pressed?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      className="menu-item"
      aria-pressed={pressed}
      onClick={(e) => {
        hideMenu(e);
        onClick();
      }}
    >
      <Icon name={icon} size={16} />
      {children}
    </button>
  );
}

function MenuLink({ icon, href, children }: { icon: IconName; href: string; children: ReactNode }) {
  return (
    <External href={href} className="menu-item" onClick={hideMenu}>
      <Icon name={icon} size={16} />
      <span>{children}</span>
      <span aria-hidden="true" className="menu-out">
        ↗
      </span>
    </External>
  );
}

/// Under the thread: the answers its reply offers, or the options of the
/// dialog it shows, and the composer, with images and dictation.
function Dock({
  s,
  detail,
  answers,
  phone,
  busy,
  error,
  onPick,
  onSend,
}: {
  s: Session;
  detail: SessionDetail | null;
  answers: string[];
  phone: boolean;
  busy: boolean;
  error: string | null;
  onPick: (n: number) => void;
  onSend: (text: string, images: string[]) => Promise<boolean>;
}) {
  const dialog = detail?.dialog ?? null;
  const blocked = dialog
    ? "A dialog is open: answer it first."
    : s.status === "running"
      ? "Running: send once the turn is done."
      : !canSend(s)
        ? "The session has stopped and takes no follow-up."
        : null;
  const c = useCompose(onSend, blocked, busy);
  const who = s.role === "captain" ? "the captain" : harnessName(s);

  return (
    <div className="dock">
      <div className="dock-in">
        {dialog ? (
          <div className="dock-dialog">
            <p className="dock-say">
              <span className="dock-say-what">Answer the dialog</span>
              <span>Presses its key in the session</span>
            </p>
            <div role="group" aria-label="Answer the dialog" className="dock-opts">
              {dialog.options.map((o) => (
                <button
                  key={o.number}
                  type="button"
                  className="dock-opt"
                  disabled={busy}
                  aria-keyshortcuts={String(o.number)}
                  onClick={() => onPick(o.number)}
                >
                  <kbd aria-hidden="true">{o.number}</kbd>
                  <span>{o.label}</span>
                </button>
              ))}
            </div>
          </div>
        ) : (
          answers.length > 0 && (
            <div role="group" aria-label="Answers" className="dock-chips">
              {answers.map((a, i) => (
                <button
                  key={a}
                  type="button"
                  className={i === 0 ? "dock-chip first" : "dock-chip"}
                  disabled={busy}
                  aria-keyshortcuts={String(i + 1)}
                  onClick={() => onPick(i + 1)}
                >
                  <kbd aria-hidden="true">{i + 1}</kbd>
                  {a}
                </button>
              ))}
              {phone ? (
                <button type="button" className="dock-chip say tint-mauve" onClick={c.dictation.begin}>
                  <Icon name="mic" size={18} />
                  Say it
                </button>
              ) : (
                <span className="dock-hint">An answer is sent as your reply</span>
              )}
            </div>
          )
        )}
        <ComposeForm
          c={c}
          id="reply-text"
          phone={phone}
          words={{
            label: `Reply to ${s.name}`,
            placeholder: phone ? `Reply to ${who}…` : `Reply to ${s.name}…`,
            to: `Replying to ${who} · ${s.name}`,
            send: "Send",
            dictate: "Dictate a reply",
          }}
        />
        {error && (
          <p className="error dock-error" role="alert">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}

/// On a phone, the page fits what the on-screen keyboard leaves of the
/// screen, so it never covers the composer: iOS shrinks the visual
/// viewport for it and pans that over the page, and the page is fixed to
/// the layout viewport.
export function useKeyboardFit(on: boolean) {
  useEffect(() => {
    const vv = window.visualViewport;
    if (!on || !vv) return;
    const root = document.documentElement;
    const clear = () => {
      delete root.dataset.keyboard;
      root.style.removeProperty("--vv-top");
      root.style.removeProperty("--vv-height");
    };
    const fit = () => {
      if (Math.abs(vv.scale - 1) > 0.01 || innerHeight - vv.height < 80) return clear();
      root.dataset.keyboard = "";
      root.style.setProperty("--vv-top", `${vv.offsetTop}px`);
      root.style.setProperty("--vv-height", `${vv.height}px`);
    };
    fit();
    vv.addEventListener("resize", fit);
    vv.addEventListener("scroll", fit);
    return () => {
      vv.removeEventListener("resize", fit);
      vv.removeEventListener("scroll", fit);
      clear();
    };
  }, [on]);
}

/// The session's screen as it is now, read once a second while shown.
export function Terminal({
  s,
  canShow,
  shown,
  onShow,
  onHide,
}: {
  s: Session;
  canShow: boolean;
  shown: (Flash & { seq: number }) | null;
  onShow: () => void;
  onHide: () => void;
}) {
  const screen = useScreen(s.id);
  const scroller = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLPreElement>(null);
  useStick(scroller, content);
  return (
    <section className="term" aria-label="Live terminal">
      <div className="term-top">
        <span className="term-live">
          <span aria-hidden="true" className="term-dot" />
          Live
        </span>
        <span className="m term-where">
          {s.projectName} · {s.branch}
        </span>
        {screen && (
          <span className="m term-size">
            {screen.cols}×{screen.height}
          </span>
        )}
        <span className="term-acts">
          {canShow && (
            <span className="flash-anchor">
              <button type="button" className="term-btn" onClick={onShow}>
                <Icon name="monitor" size={14} />
                Open in TUI
              </button>
              <FlashSpot flash={shown} />
            </span>
          )}
          <button type="button" className="term-btn sq" aria-label="Hide the live terminal" title="Hide the live terminal (T)" onClick={onHide}>
            <Icon name="panelRight" size={15} />
          </button>
        </span>
      </div>
      <div className="term-scroll" ref={scroller}>
        <pre
          ref={content}
          className={screen ? "term-screen" : "term-screen waiting"}
          tabIndex={0}
          aria-label="Terminal screen"
          style={screen ? ({ "--cols": screen.cols } as CSSProperties) : undefined}
        >
          {screen ? screen.rows.join("\n").trimEnd() : "Reading the screen…"}
        </pre>
      </div>
    </section>
  );
}

/// An assigned issue no session works on: what Linear says of it, a link
/// to it there, and Start, which asks on a card of its own first.
export function IssueDetail({
  issue: i,
  state,
  now,
  asking,
  busy,
  error,
  layout,
  back,
  onBack,
  onAsk,
  onStart,
  pick,
  note,
  onSay,
  banner,
}: {
  issue: Issue;
  state: State;
  now: number;
  /// Start's question is open.
  asking: boolean;
  busy: boolean;
  error: string | null;
  layout: Layout;
  back: string;
  onBack: () => void;
  onAsk: (open: boolean) => void;
  onStart: (project: string, again: boolean) => void;
  /// The orchestrator picked it.
  pick: FocusItem | null;
  note: string | null;
  onSay: SayOf | null;
  /// What the page says across its top.
  banner: ReactNode;
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
  const phone = layout === "phone";
  const canStart = !!offer.pick && !asking;
  const start = canStart && (
    <button
      ref={startButton}
      type="button"
      className={phone ? "dock-start" : "btn primary"}
      aria-keyshortcuts="s"
      title="Hands the issue to a project's captain, which starts a worker on it."
      onClick={() => onAsk(true)}
    >
      <Icon name="play" size={13} className="filled" />
      {phone ? `Start ${i.key}` : "Start"}
      {!phone && <kbd aria-hidden="true">S</kbd>}
    </button>
  );
  const linear = (
    <External className={phone ? "sess-link" : "btn"} href={i.url}>
      Linear ↗
    </External>
  );
  const started = i.stateType === "started";
  return (
    <main className="sess issue-page" data-layout={layout}>
      <section className="sess-main" aria-label={`${i.key} ${i.title}`}>
        <header className="sess-head">
          {phone ? (
            <div className="sess-nav">
              <BackLink back={back} onBack={onBack} />
              <span className="sess-gap" />
              {linear}
            </div>
          ) : (
            layout === "single" && <BackLink back={back} onBack={onBack} />
          )}
          {banner}
          <div className="sess-top">
            <h1 className="sess-title">{i.title}</h1>
            {!phone && <span className={started ? "state-pill tint-yellow" : "state-pill tint-plain"}>{i.state}</span>}
            {!phone && (i.priority === 1 || i.priority === 2) && (
              <span className={i.priority === 1 ? "state-pill tint-peach" : "state-pill tint-plain"}>{priorityLabel(i.priority)}</span>
            )}
            {!phone && (
              <div className="sess-acts">
                {start}
                {linear}
              </div>
            )}
          </div>
          {phone ? (
            <>
              <p className="sess-sub">
                {i.key}
                {i.project && ` · ${i.project}`}
              </p>
              <div className="life-chips" role="group" aria-label="Issue, session and pull request">
                <span className={started ? "lchip tint-yellow" : "lchip"}>{i.state}</span>
                <span className="lchip-pair">
                  <ChipStep />
                  <span className="lchip">{i.started ? `handed to ${i.started.projectName}` : "No session yet"}</span>
                </span>
              </div>
            </>
          ) : (
            <nav className="life" aria-label="Issue, session and pull request">
              <External href={i.url} className="life-step life-issue" aria-current="step">
                <span className="life-head">
                  Issue
                  <span aria-hidden="true" className={started ? "sdot yellow" : "sdot hollow"} />
                  <span className="life-state">{i.state}</span>
                </span>
                <span className="life-main">
                  <span className="m life-key">{i.key}</span>
                  <span className="life-title">{i.title}</span>
                </span>
              </External>
              <Step />
              <div className="life-step life-session">
                <span className="life-head">Session</span>
                <span className="life-main life-none">
                  {i.started
                    ? i.started.held
                      ? `Handed to ${i.started.projectName}, brief held`
                      : `Handed to ${i.started.projectName} ${ago(i.started.at, now)}`
                    : "No session yet"}
                </span>
              </div>
              <Step />
              <div className="life-step life-pr">
                <span className="life-head">PR</span>
                <span className="life-main life-none">No PR yet</span>
              </div>
            </nav>
          )}
        </header>
        <div className="sess-scroll">
          <div className="sess-col issue-col">
            <WhyBanner pick={pick} rule={false} note={note} onSay={onSay} />
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
            {i.started ? (
              <p className="issue-line handed">{handedLine(i.started, now)}</p>
            ) : (
              <p className="issue-line">Assigned to you, and no session works on it.</p>
            )}
            {i.started?.moveFailed && (
              <p className="issue-line move-failed">It was not moved to its started state in Linear: {i.started.moveFailed}</p>
            )}
            {!offer.pick && (
              <p className="issue-line">No project reads its issues from Linear's {i.team} team, so Start has nowhere to hand it.</p>
            )}
            <dl className="issue-facts" aria-label="In Linear">
              <div>
                <dt>Key</dt>
                <dd>
                  <External href={i.url} className="m">
                    {i.key} ↗
                  </External>
                </dd>
              </div>
              <div>
                <dt>State</dt>
                <dd>
                  <span aria-hidden="true" className={started ? "sdot yellow" : "sdot hollow"} />
                  {i.state}
                </dd>
              </div>
              <div>
                <dt>Priority</dt>
                <dd>
                  <Priority p={i.priority} />
                  {priorityLabel(i.priority)}
                </dd>
              </div>
              <div>
                <dt>Project</dt>
                <dd>{i.project ?? <span className="facts-none">none</span>}</dd>
              </div>
              <div>
                <dt>Updated</dt>
                <dd>
                  {i.updatedAt > 0 ? (
                    <span>
                      {when(i.updatedAt, now)} <span className="facts-none">· {ago(i.updatedAt, now)}</span>
                    </span>
                  ) : (
                    <span className="facts-none">unknown</span>
                  )}
                </dd>
              </div>
              {i.branchName && (
                <div>
                  <dt>Branch</dt>
                  <dd>
                    <span className="m">{i.branchName}</span>
                  </dd>
                </div>
              )}
            </dl>
          </div>
        </div>
        {phone && start && (
          <div className="dock">
            <div className="dock-in">{start}</div>
          </div>
        )}
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

/// Start's question, at the top of an issue's page: the project it goes
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
  // While its Start is under way the card says what it asked: the state the
  // hand-off changes as it goes is not its answer.
  const asked = useRef({ state, issue: i });
  if (!busy) asked.current = { state, issue: i };
  const { state: was, issue: then } = asked.current;
  const again = !!then.started;
  return (
    <div className="start-ask" role="group" aria-labelledby="start-ask">
      <p id="start-ask" className="start-q">
        {again
          ? `${i.key} was handed to the captain of ${then.started!.projectName} already. Start it again in ${project.name}?`
          : `Start ${i.key} in ${project.name}?`}
      </p>
      {offered.length > 1 && (
        <div className="start-pick">
          <span id="start-projects">Project</span>
          <div role="group" aria-labelledby="start-projects" className="start-projects" onKeyDown={arrowsMove}>
            {offered.map((p) => (
              <button key={p.id} type="button" aria-pressed={p.id === project.id} onClick={() => setPicked(p.id)}>
                {p.name}
              </button>
            ))}
          </div>
        </div>
      )}
      <p className="start-says">{handOff(was, project)}</p>
      <p className="start-says">
        {then.stateType === "started"
          ? `It is ${then.state} in Linear already, and stays there.`
          : `In Linear the issue moves from ${then.state} to its team's first started state.`}
      </p>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <div className="start-acts">
        <button
          type="button"
          className="btn primary"
          disabled={busy}
          aria-busy={busy || undefined}
          onClick={() => onStart(project.id, again)}
        >
          {busy ? "Starting…" : again ? `Start ${i.key} again` : `Start ${i.key}`}
        </button>
        <button ref={cancelRef} type="button" className="btn" onClick={onCancel}>
          Cancel
        </button>
        <span className="start-says">The worker shows on the board once the captain starts it.</span>
      </div>
    </div>
  );
}
