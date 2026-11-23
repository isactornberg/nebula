import MarkdownIt from "markdown-it";
import {
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
  type AnchorHTMLAttributes,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
} from "react";
import { routePath, type Flash, type Route, type Tone } from "./model";

// Raw HTML in Markdown is shown as text, never parsed (`html: false`), and
// markdown-it drops javascript: and data: links.
export const md = new MarkdownIt({ html: false, linkify: true });
const linkOpen = md.renderer.rules.link_open ?? ((t, i, o, _e, self) => self.renderToken(t, i, o));
md.renderer.rules.link_open = (tokens, idx, options, env, self) => {
  tokens[idx]!.attrSet("target", "_blank");
  tokens[idx]!.attrSet("rel", "noreferrer");
  return linkOpen(tokens, idx, options, env, self);
};

export function isTextField(t: HTMLElement): boolean {
  return t.isContentEditable || t.matches("input, textarea, select");
}

/// A menu, a dialog or dictation is open over the page, so the page's own
/// keys wait until it closes.
export function overlayOpen(doc: { querySelector(selectors: string): Element | null } = document): boolean {
  try {
    if (doc.querySelector(":popover-open")) return true;
  } catch {
    // A browser without popovers has none open.
  }
  return !!doc.querySelector("dialog[open], .dict");
}

/// The arrow keys move between the buttons of a group of positions, as a
/// switch's or Start's project choice draws them.
export function arrowsMove(e: KeyboardEvent<HTMLElement>) {
  if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
  e.preventDefault();
  const buttons = [...e.currentTarget.querySelectorAll("button")];
  const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
  buttons[(at + (e.key === "ArrowRight" ? 1 : -1) + buttons.length) % buttons.length]?.focus();
}

export function Markdown({ text }: { text: string }) {
  const html = useMemo(() => md.render(text), [text]);
  return <div className="md" dangerouslySetInnerHTML={{ __html: html }} />;
}

/// A real link to `to`: a plain click calls `onGo` in place of loading the
/// page, a modified one (new tab, new window) is the browser's.
export function RouteLink({
  to,
  onGo,
  ...rest
}: { to: Route; onGo: () => void } & AnchorHTMLAttributes<HTMLAnchorElement>) {
  const click = (e: MouseEvent) => {
    if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    e.preventDefault();
    onGo();
  };
  return <a {...rest} href={routePath(to)} onClick={click} />;
}

export function Dot({ tone }: { tone: Tone }) {
  return <span aria-hidden="true" className={`sdot ${tone}`} />;
}

/// nebula's mark, the gas cloud. The favicons in index.html and pair.rs and
/// the PNG icons in public/ are this same drawing.
export function Mark({ size = 16 }: { size?: number }) {
  return (
    <svg aria-hidden="true" width={size} height={size} viewBox="2 2 20 20">
      <circle cx="9.5" cy="10" r="6.2" fill="#c6a0f6" opacity="0.9" />
      <circle cx="15.2" cy="11" r="5.2" fill="#8aadf4" opacity="0.8" />
      <circle cx="11.6" cy="15.6" r="4.6" fill="#f5bde6" opacity="0.8" />
      <circle cx="15.6" cy="8.2" r="1.1" fill="#f4f6ff" />
    </svg>
  );
}

const PR_FORK = (
  <>
    <circle cx="6" cy="6" r="2.5" />
    <circle cx="6" cy="18" r="2.5" />
    <circle cx="18" cy="18" r="2.5" />
    <path d="M6 8.5v7" />
  </>
);

/// The page's line icons, drawn on a 24 px grid in the text's colour.
const ICONS = {
  plus: <path d="M12 5v14M5 12h14" />,
  search: (
    <>
      <circle cx="11" cy="11" r="7" />
      <path d="M20 20l-3.5-3.5" />
    </>
  ),
  inbox: (
    <>
      <path d="M22 12h-6l-2 3h-4l-2-3H2" />
      <path d="M5.5 5.1L2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.5-6.9A2 2 0 0 0 16.8 4H7.2a2 2 0 0 0-1.7 1.1z" />
    </>
  ),
  running: <path d="M22 12h-4l-3 9L9 3l-3 9H2" />,
  issues: (
    <>
      <circle cx="12" cy="12" r="9" />
      <circle cx="12" cy="12" r="2.2" />
    </>
  ),
  folder: <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />,
  clock: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l3 2" />
    </>
  ),
  skills: (
    <>
      <path d="M12 3l9 5-9 5-9-5 9-5z" />
      <path d="M3 13l9 5 9-5" />
    </>
  ),
  deck: (
    <>
      <rect x="3" y="3" width="7" height="7" rx="1.5" />
      <rect x="14" y="3" width="7" height="7" rx="1.5" />
      <rect x="3" y="14" width="7" height="7" rx="1.5" />
      <rect x="14" y="14" width="7" height="7" rx="1.5" />
    </>
  ),
  moon: <path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z" />,
  sun: (
    <>
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
    </>
  ),
  refresh: (
    <>
      <path d="M21 12a9 9 0 1 1-2.64-6.36" />
      <path d="M21 3v6h-6" />
    </>
  ),
  sparkle: <path d="M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8z" />,
  more: (
    <>
      <circle cx="5" cy="12" r="1.2" />
      <circle cx="12" cy="12" r="1.2" />
      <circle cx="19" cy="12" r="1.2" />
    </>
  ),
  play: <path d="M7 4.5v15l12-7.5z" />,
  training: <path d="M6 7v10M3 9v6M18 7v10M21 9v6M6 12h12" />,
  todos: (
    <>
      <path d="M10 6h10M10 12h10M10 18h10" />
      <path d="M3 6l1.5 1.5L7 5M3 12l1.5 1.5L7 11M3 18l1.5 1.5L7 17" />
    </>
  ),
  prOpen: (
    <>
      {PR_FORK}
      <path d="M18 15.5V9a3 3 0 0 0-3-3h-3" />
      <path d="M14 3.5L11.5 6 14 8.5" />
    </>
  ),
  prMerged: (
    <>
      {PR_FORK}
      <path d="M18 15.5V12a6 6 0 0 0-6-6H8.5" />
    </>
  ),
  done: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M8 12.5l2.7 2.7L16 9.5" />
    </>
  ),
  today: (
    <>
      <rect x="3" y="5" width="18" height="16" rx="2" />
      <path d="M3 10h18M8 3v4M16 3v4" />
    </>
  ),
  chevronDown: <path d="M6 9l6 6 6-6" />,
  terminal: (
    <>
      <path d="M4 17l6-5-6-5" />
      <path d="M12 19h8" />
    </>
  ),
  close: <path d="M6 6l12 12M18 6L6 18" />,
  anchor: (
    <>
      <circle cx="12" cy="5" r="3" />
      <path d="M12 22V8" />
      <path d="M5 12H2a10 10 0 0 0 20 0h-3" />
    </>
  ),
  bot: (
    <>
      <rect x="4" y="8" width="16" height="12" rx="3" />
      <path d="M12 8V4" />
      <path d="M9 13v2M15 13v2" />
    </>
  ),
  monitor: (
    <>
      <rect x="2" y="4" width="20" height="14" rx="2" />
      <path d="M8 21h8M12 18v3" />
    </>
  ),
  eye: (
    <>
      <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z" />
      <circle cx="12" cy="12" r="3" />
    </>
  ),
  mic: (
    <>
      <rect x="9" y="2" width="6" height="12" rx="3" />
      <path d="M19 10v1a7 7 0 0 1-14 0v-1" />
      <path d="M12 18v4" />
    </>
  ),
  image: (
    <>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <circle cx="9" cy="10" r="2" />
      <path d="M21 16l-5-5-9 9" />
    </>
  ),
  arrowUp: <path d="M12 19V5M5 12l7-7 7 7" />,
  reply: (
    <>
      <path d="M9 14L4 9l5-5" />
      <path d="M4 9h10a6 6 0 0 1 6 6v4" />
    </>
  ),
  phone: (
    <>
      <rect x="7" y="2" width="10" height="20" rx="2.5" />
      <path d="M11 18h2" />
    </>
  ),
  check: <path d="M5 12l5 5 9-10" />,
  chevronRight: <path d="M9 6l6 6-6 6" />,
  chevronLeft: <path d="M15 18l-6-6 6-6" />,
  shield: <path d="M12 3l7 3v5c0 4.5-3 8-7 10-4-2-7-5.5-7-10V6z" />,
  archive: (
    <>
      <rect x="2" y="3" width="20" height="5" rx="1" />
      <path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8M10 12h4" />
    </>
  ),
  panelRight: (
    <>
      <rect x="3" y="3" width="18" height="18" rx="2" />
      <path d="M15 3v18" />
      <path d="M8 9l3 3-3 3" />
    </>
  ),
} satisfies Record<string, ReactNode>;

export type IconName = keyof typeof ICONS;

export const isIcon = (name: string): name is IconName => Object.hasOwn(ICONS, name);

export function Icon({ name, size = 20, className }: { name: IconName; size?: number; className?: string }) {
  return (
    <svg
      aria-hidden="true"
      className={className ? `icon ${className}` : "icon"}
      data-icon={name}
      width={size}
      height={size}
      viewBox="0 0 24 24"
    >
      {ICONS[name]}
    </svg>
  );
}

/// Whether the screen matches `query` now, read again as it changes; never,
/// where there is no screen to ask.
export function useMedia(query: string): boolean {
  return useSyncExternalStore(
    (changed) => {
      const m = matchMedia(query);
      m.addEventListener("change", changed);
      return () => m.removeEventListener("change", changed);
    },
    () => matchMedia(query).matches,
    () => false,
  );
}

export function External(props: AnchorHTMLAttributes<HTMLAnchorElement>) {
  return <a {...props} target="_blank" rel="noreferrer" />;
}

/// A flash and the control it is said under: `follow`, `refresh`, or
/// `terminal:<session id>`.
export type Said = Flash & { at: string; seq: number };

/// A word said for a moment under the control it is about. The live
/// region stays in the page, so a screen reader hears each word as it
/// comes; `seq` tells two of the same words apart.
export function FlashSpot({ flash }: { flash: (Flash & { seq: number }) | null }) {
  return (
    <span className="flash" role="status">
      {flash && (
        <span key={flash.seq} className={flash.error ? "flash-text error" : "flash-text"}>
          {flash.text}
        </span>
      )}
    </span>
  );
}

/// The clock, read again every 15 s.
export function useNow(): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 15_000);
    return () => clearInterval(t);
  }, []);
  return now;
}
