import type { ReactNode } from "react";
import { IS_MAC, MOD } from "./Compose";
import type { ListView, Route, Section } from "./model";
import { Notify } from "./Notify";
import { onThisMachine, PhonesButton } from "./Phones";
import { VIEW_TITLE } from "./Side";
import { toggleTheme, useTheme } from "./theme";
import { Icon, Mark, RouteLink, type IconName } from "./ui";

const VIEW_ICON: Record<ListView, IconName> = {
  inbox: "inbox",
  running: "running",
  issues: "issues",
  projects: "folder",
  today: "today",
  more: "more",
};

type Go = {
  section: Section;
  view: ListView;
  /// How many rows need you, on Inbox's badge.
  needsYou: number;
  onView: (v: ListView) => void;
  onGo: (to: Route) => void;
  /// New session is open.
  making: boolean;
  onPalette: () => void;
  onPhones: () => void;
};

function Badge({ n }: { n: number }) {
  if (n === 0) return null;
  return (
    <span className="badge-n">
      <span className="sr">, </span>
      {n}
      <span className="sr"> {n === 1 ? "needs" : "need"} you</span>
    </span>
  );
}

/// The list views the rail and the tab bar go to: each goes Home, with the
/// list on that view.
function ViewLink({ v, className, go, children }: { v: ListView; className: string; go: Go; children?: ReactNode }) {
  return (
    <RouteLink
      to={{ kind: "idle" }}
      onGo={() => go.onView(v)}
      className={className}
      aria-current={go.section === "attention" && go.view === v ? "page" : undefined}
    >
      <Icon name={VIEW_ICON[v]} size={className === "tab" ? 23 : 20} />
      <span className="item-name">{VIEW_TITLE[v]}</span>
      {v === "inbox" && <Badge n={go.needsYou} />}
      {children}
    </RouteLink>
  );
}

function ThemeButton({ className }: { className: string }) {
  const theme = useTheme();
  const to = theme === "dark" ? "light" : "dark";
  return (
    <button type="button" className={className} aria-label={`Switch to the ${to} theme`} onClick={toggleTheme}>
      <Icon name={to === "light" ? "sun" : "moon"} size={20} />
      <span className="item-name">{to === "light" ? "Light" : "Dark"}</span>
    </button>
  );
}

/// The rail down a wider screen's left edge: home, the list's views, the
/// other pages, and at its foot Phones or this device's notifications and
/// the theme. Below 932 px, where Home is not beside the list, the day is
/// a view of the list too.
export function Rail({ go, single, connected }: { go: Go; single: boolean; connected: boolean }) {
  const views: ListView[] = single ? ["inbox", "running", "issues", "today", "projects"] : ["inbox", "running", "issues", "projects"];
  return (
    <nav className="rail" aria-label="Main">
      <RouteLink to={{ kind: "idle" }} onGo={() => go.onView("inbox")} className="rail-mark" aria-label="nebula home">
        <Mark size={30} />
      </RouteLink>
      <RouteLink
        to={{ kind: "new" }}
        onGo={() => go.onGo({ kind: "new" })}
        className="rail-new"
        aria-current={go.making ? "page" : undefined}
        aria-keyshortcuts="n"
        title="New session (N)"
      >
        <span className="rail-new-tile">
          <Icon name="plus" size={18} />
        </span>
        <span className="item-name">New</span>
      </RouteLink>
      <button
        type="button"
        className="rail-item"
        aria-haspopup="dialog"
        aria-keyshortcuts={IS_MAC ? "Meta+K" : "Control+K"}
        title={`Search or jump (${MOD}K)`}
        onClick={go.onPalette}
      >
        <Icon name="search" />
        <span className="item-name">
          <span aria-hidden="true">{MOD}K</span>
          <span className="sr">Search or jump</span>
        </span>
      </button>
      <span aria-hidden="true" className="rail-rule" />
      {views.map((v) => (
        <ViewLink key={v} v={v} className="rail-item" go={go} />
      ))}
      <RouteLink
        to={{ kind: "schedules" }}
        onGo={() => go.onGo({ kind: "schedules" })}
        className="rail-item"
        aria-current={go.section === "schedules" ? "page" : undefined}
      >
        <Icon name="clock" />
        <span className="item-name">Schedules</span>
      </RouteLink>
      <RouteLink
        to={{ kind: "skills" }}
        onGo={() => go.onGo({ kind: "skills" })}
        className="rail-item"
        aria-current={go.section === "skills" ? "page" : undefined}
      >
        <Icon name="skills" />
        <span className="item-name">Skills</span>
      </RouteLink>
      {/* A page of its own, outside the app's routes, so a plain link. */}
      <a href="/deck" className="rail-item">
        <Icon name="deck" />
        <span className="item-name">Deck</span>
      </a>
      <span className="rail-gap" />
      {!connected && (
        <span className="rail-offline" role="status">
          <span aria-hidden="true" className="sdot red" />
          Offline
          <span className="sr">: disconnected from nebula</span>
        </span>
      )}
      {onThisMachine() ? <PhonesButton onOpen={go.onPhones} /> : <Notify onGo={go.onGo} />}
      <ThemeButton className="rail-item" />
    </nav>
  );
}

/// A phone's top bar: the mark, the palette, and Phones or this phone's
/// notifications; under it, what needs saying across the page.
export function PhoneTop({
  onGo,
  onSearch,
  onPhones,
  children,
}: {
  onGo: (to: Route) => void;
  onSearch: () => void;
  onPhones: () => void;
  children: ReactNode;
}) {
  return (
    <header className="ptop">
      <div className="ptop-bar">
        <RouteLink to={{ kind: "idle" }} onGo={() => onGo({ kind: "idle" })} className="ptop-mark">
          <Mark size={28} />
          <span>nebula</span>
        </RouteLink>
        <button type="button" className="round" aria-label="Search or jump" aria-haspopup="dialog" onClick={onSearch}>
          <Icon name="search" size={19} />
        </button>
        {onThisMachine() ? <PhonesButton onOpen={onPhones} /> : <Notify onGo={onGo} />}
      </div>
      {children}
    </header>
  );
}

/// A phone's tab bar. More holds the pages and the views the bar has no
/// room for, so it is the tab of each.
export function Tabs({ go, todayTone }: { go: Go; todayTone: "ok" | "late" | "error" | null }) {
  const more = go.section !== "attention" || go.view === "projects";
  return (
    <nav className="tabs-bar" aria-label="Sections">
      <ViewLink v="inbox" className="tab" go={go} />
      <ViewLink v="running" className="tab" go={go} />
      <ViewLink v="issues" className="tab" go={go} />
      <ViewLink v="today" className="tab" go={go}>
        {todayTone && todayTone !== "ok" && <span aria-hidden="true" className={`tab-dot ${todayTone}`} />}
      </ViewLink>
      <RouteLink
        to={{ kind: "idle" }}
        onGo={() => go.onView("more")}
        className="tab"
        aria-current={(go.section === "attention" && go.view === "more") || more ? "page" : undefined}
      >
        <Icon name="more" size={23} />
        <span className="item-name">More</span>
      </RouteLink>
    </nav>
  );
}

/// More, on a phone: `first`, the orchestrator's thread, the pages and
/// views the tab bar has no room for, the theme, and what the list's foot
/// holds on a wider screen.
export function More({ go, first, children }: { go: Go; first: ReactNode; children: ReactNode }) {
  return (
    <div className="more-view">
      <nav className="more-list" aria-label="More">
        {first}
        <ViewLink v="projects" className="more-item" go={go} />
        <RouteLink to={{ kind: "schedules" }} onGo={() => go.onGo({ kind: "schedules" })} className="more-item">
          <Icon name="clock" />
          <span className="item-name">Schedules</span>
        </RouteLink>
        <RouteLink to={{ kind: "skills" }} onGo={() => go.onGo({ kind: "skills" })} className="more-item">
          <Icon name="skills" />
          <span className="item-name">Skills</span>
        </RouteLink>
        <a href="/deck" className="more-item">
          <Icon name="deck" />
          <span className="item-name">Deck</span>
        </a>
        <ThemeButton className="more-item" />
      </nav>
      {children}
    </div>
  );
}
