import { useEffect, useImperativeHandle, useMemo, useRef, useState, type KeyboardEvent, type Ref, type RefObject } from "react";
import { api, useCompare, useSkillDetail, useSkills, useSkillUsage, type Read } from "./api/client";
import type {
  Project,
  Skill,
  SkillAct,
  SkillCli,
  SkillCheck,
  SkillCompare,
  SkillControl,
  SkillDetail,
  SkillDone,
  SkillFix,
  SkillProblem,
  SkillProblemKind,
  Skills,
  SkillSetting,
  SkillTo,
  SkillUsage,
  SkillWrite,
} from "./api/contract";
import { clock, moveCursor, type Route } from "./model";
import {
  actLabel,
  addedText,
  allGroups,
  and,
  asEdited,
  asSaved,
  bestItem,
  CLI_TITLE,
  controlOf,
  controlsText,
  copyName,
  count,
  cureOf,
  differing,
  doneText,
  draftOf,
  filterTree,
  groupUsed,
  leaveAction,
  linesOf,
  placeOf,
  POSITIONS,
  problemCount,
  problemsOf,
  removeAsk,
  rowsOf,
  rowUsed,
  saveLabel,
  sessionGets,
  SETTING_TITLE,
  skillCount,
  skillKey,
  skillsSummary,
  skillTree,
  STATE_TITLE,
  switchText,
  tidyCandidates,
  tilde,
  usedLabel,
  usedText,
  usesOf,
  useSummary,
  valueText,
  visibleItems,
  whereItLoads,
  writtenText,
  type Draft,
  type Group,
  type Item,
  type Section,
  type SkillAction,
  type Used,
  type Uses,
} from "./skills";
import { Dot, External, Markdown, md, RouteLink } from "./ui";

const OPEN_KEY = "nebula.skills.open";

function readOpen(): Set<string> {
  try {
    const keys: unknown = JSON.parse(localStorage.getItem(OPEN_KEY) ?? "[]");
    return new Set(Array.isArray(keys) ? keys.filter((k) => typeof k === "string") : []);
  } catch {
    return new Set();
  }
}

function saveOpen(open: Set<string>) {
  try {
    localStorage.setItem(OPEN_KEY, JSON.stringify([...open]));
  } catch {
    // Folded again on the next load, then.
  }
}

function isTextField(t: HTMLElement): boolean {
  return t.isContentEditable || t.matches("input, textarea, select");
}

/// What a write said, and at which control: `skill:<id>:<cli>`,
/// `plugin:<id>`, `fix:<fix id>` or `tidy`.
export interface Said {
  at: string;
  text: string;
  error: boolean;
  /// What an act left alone, as sentences.
  skipped?: string[];
  /// The token that undoes the act.
  undo?: string | null;
}

/// The page's writes: which control is writing, what the last one said,
/// and the calls that make one.
export interface Writes {
  busy: string | null;
  said: Said | null;
  /// `run` at `at`, saying `done` before the file it wrote, and `then`
  /// once the list is read again.
  write: (at: string, run: () => Promise<Read<SkillWrite>>, opts?: { done?: string; then?: (w: SkillWrite) => void }) => void;
  /// An act on a fix, a save, or its Undo, at `at`, saying what it did
  /// once the list is read again and then calling `then`; `failed` says
  /// what went wrong in its place, and `reread` reads the list again after
  /// a refusal too, for an act refused because a file moved on.
  act: (
    at: string,
    run: () => Promise<Read<SkillDone>>,
    opts?: { then?: () => void; failed?: (error: string) => void; reread?: boolean },
  ) => void;
  /// The removal waiting on its confirm, as `<at>:<act>`: one at a time.
  asking: string | null;
  ask: (asking: string | null) => void;
}

/// The tidy-up once done: the skills it made Slash menu only, and how each
/// stood before.
export interface Tidied {
  names: string[];
  before: SkillTo[];
}

const SECTION: Record<Section["key"], { title: string; note: string }> = {
  global: { title: "Global", note: "loads in every project" },
  projects: { title: "Projects", note: "load only in that project" },
};

/// The Skills page: every skill on the left, folded by where it loads and
/// comes from, and the open one (or what needs fixing) on the right. Every
/// key does what a click on the same thing does.
export function SkillsPage({
  openId,
  fixes,
  projects,
  onGo,
}: {
  openId: string | null;
  /// On the screen of what needs fixing, which a phone opens from the list.
  fixes: boolean;
  projects: Project[];
  onGo: (to: Route, replace?: boolean) => void;
}) {
  const { got, reload } = useSkills();
  const list = got?.ok ? got.value : null;
  const home = list?.home ?? "";
  const [query, setQuery] = useState("");
  const [cli, setCli] = useState<SkillCli | null>(null);
  const [cursor, setCursor] = useState<string | null>(null);
  const [open, setOpen] = useState(readOpen);
  // While a filter is typed, the groups it leaves are open, and folding
  // one lasts until the filter changes.
  const [filterOpen, setFilterOpen] = useState<Set<string>>(new Set());
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [said, setSaid] = useState<Said | null>(null);
  const [tidied, setTidied] = useState<Tidied | null>(null);
  const [asking, setAsking] = useState<string | null>(null);
  const filter = useRef<HTMLInputElement>(null);
  /// A write, one at a time, saying where it was asked what it did once
  /// the list is read again.
  const write: Writes["write"] = (at, run, opts = {}) => {
    if (busy) return;
    setBusy(at);
    void run().then(async (r) => {
      if (r.ok) {
        await reload();
        const text = [r.value.changed ? opts.done : null, writtenText(r.value)].filter(Boolean).join(" ");
        setSaid({ at, text, error: false });
        opts.then?.(r.value);
      } else {
        setSaid({ at, text: r.error, error: true });
      }
      setBusy(null);
    });
  };
  const actOn: Writes["act"] = (at, run, opts = {}) => {
    if (busy) return;
    setBusy(at);
    setAsking(null);
    void run().then(async (r) => {
      if (r.ok) {
        await reload();
        setSaid({ at, text: doneText(r.value), error: false, skipped: r.value.skipped, undo: r.value.undo });
        opts.then?.();
      } else if (opts.failed) {
        opts.failed(r.error);
      } else {
        if (opts.reread) await reload();
        setSaid({ at, text: r.error, error: true });
      }
      setBusy(null);
    });
  };
  const writes: Writes = { busy, said, write, act: actOn, asking, ask: setAsking };
  const spaces = projects.map((p) => `${p.name}\u0000${p.space ?? ""}`).join("\n");
  const full = useMemo(() => (list ? skillTree(list.skills, home, projects) : []), [list, spaces]);
  const byCli = useMemo(() => (list && cli ? skillTree(list.skills, home, projects, cli) : full), [full, cli]);
  const shown = useMemo(() => filterTree(byCli, query), [byCli, query]);
  const filtering = query.trim() !== "";
  const unfolded = filtering ? filterOpen : open;
  const items = useMemo(() => visibleItems(shown, unfolded), [shown, unfolded]);
  const skill = list?.skills.find((s) => s.id === openId) ?? null;
  const detail = useSkillDetail(skill?.id ?? null, list);
  // What the editor holds for each skill with changes not saved, so
  // leaving its page loses none of them.
  const [drafts] = useState(() => new Map<string, Draft>());
  const pane = useRef<{ edit: () => void }>(null);
  const usage = useSkillUsage(list);
  const uses = useMemo(() => usesOf(usage?.ok ? usage.value : null), [usage]);
  // Claude's use is the only one read, so with no Claude there is no column.
  const used: Used | null = list?.clis.includes("claude") ? { uses, now: Date.now() } : null;

  useEffect(() => {
    if (list && openId && !skill) {
      onGo({ kind: "skills" }, true);
      setNote(`No skill with id ${openId}.`);
    }
  }, [list, openId, skill, onGo]);

  // The skill that opens, by whatever means, is on screen in the list.
  useEffect(() => {
    if (!openId || !list) return;
    setNote(null);
    const at = placeOf(shown, openId) ?? placeOf(full, openId);
    if (!at) return;
    const keys = at.path.map((g) => g.key);
    const more = (s: Set<string>) => (keys.every((k) => s.has(k)) ? s : new Set([...s, ...keys]));
    // Open in the list as it is after the filter too.
    setOpen(more);
    if (filtering) setFilterOpen(more);
    if (!cursor?.endsWith(`/${openId}`)) setCursor(`${keys.at(-1)}/${openId}`);
  }, [openId, list === null]);

  useEffect(() => saveOpen(open), [open]);

  useEffect(() => {
    const unsaved = (e: BeforeUnloadEvent) => {
      if (drafts.size > 0) e.preventDefault();
    };
    addEventListener("beforeunload", unsaved);
    return () => removeEventListener("beforeunload", unsaved);
  }, [drafts]);

  // A confirm is for the screen it was asked on.
  useEffect(() => setAsking(null), [openId, fixes]);

  useEffect(() => {
    document.title = skill ? `${skill.name} · Skills · nebula` : fixes ? "Needs fixing · Skills · nebula" : "Skills · nebula";
  }, [skill?.name, fixes]);

  const openSkill = (id: string, key?: string) => {
    if (key) setCursor(key);
    onGo({ kind: "skill", id });
  };
  const fold = (key: string, to: boolean) =>
    (filtering ? setFilterOpen : setOpen)((s) => {
      if (s.has(key) === to) return s;
      const next = new Set(s);
      if (to) next.add(key);
      else next.delete(key);
      return next;
    });
  const onQuery = (q: string) => {
    setQuery(q);
    if (!q.trim()) return;
    const left = filterTree(byCli, q);
    const keys = new Set(allGroups(left).map((g) => g.key));
    setFilterOpen(keys);
    setCursor(bestItem(visibleItems(left, keys), q)?.key ?? null);
  };
  /// What every key on the page does, and what the pointer does through the
  /// same calls.
  const act = (action: SkillAction) => {
    const at = items.findIndex((i) => i.key === cursor);
    const item: Item | undefined = items[at];
    switch (action.kind) {
      case "move": {
        const next = moveCursor(at < 0 ? null : at, action.delta, items.length);
        return setCursor(next === null ? null : items[next]!.key);
      }
      case "edit":
        return pane.current?.edit();
      case "fold": {
        if (!item) return;
        if (item.kind === "group" && unfolded.has(item.key) !== action.open) return fold(item.key, action.open);
        if (!action.open && item.parent) setCursor(item.parent);
        return;
      }
      case "open": {
        const target = item ?? (filtering ? bestItem(items, query) : undefined);
        if (!target) return;
        if (target.kind === "group") return fold(target.key, !unfolded.has(target.key));
        return openSkill(target.row.skill.id, target.key);
      }
      case "back":
        return openId || fixes ? onGo({ kind: "skills" }) : setCursor(null);
      case "filter":
        return filter.current?.focus();
      case "clear":
        return setQuery("");
      case "leave":
        return filter.current?.blur();
    }
  };

  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.defaultPrevented || e.isComposing) return;
      if (asking && e.key === "Escape") {
        e.preventDefault();
        return setAsking(null);
      }
      const t = e.target instanceof HTMLElement ? e.target : null;
      if (t && isTextField(t)) {
        if (e.key === "Escape") t.blur();
        return;
      }
      const action = skillKey(e, false, query);
      if (!action) return;
      // Enter on a link or button is that control's own click.
      if (action.kind === "open" && t?.closest("a[href], button")) return;
      e.preventDefault();
      act(action);
    };
    addEventListener("keydown", onKey);
    return () => removeEventListener("keydown", onKey);
  });

  const filterKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.nativeEvent.isComposing) return;
    const action = skillKey(e, true, query);
    if (!action) return;
    e.preventDefault();
    act(action);
  };

  const gets = (g: Group) => {
    if (!list || g.project === null) return null;
    const each = list.clis.map((c) => `${CLI_TITLE[c]} ${sessionGets(list.skills, g.project!, g.space, c)}`);
    return [`Sessions get ${each.join(", ")}.`, addedText(g)].filter(Boolean).join(" ");
  };

  return (
    <>
      <SkillsSide
        got={got}
        sections={shown}
        open={unfolded}
        cursor={cursor}
        openId={openId}
        query={query}
        cli={cli}
        filter={filter}
        gets={gets}
        used={used}
        onQuery={onQuery}
        onFilterKey={filterKey}
        onCli={setCli}
        onFold={(key) => {
          setCursor(key);
          fold(key, !unfolded.has(key));
        }}
        onOpen={openSkill}
        onFixes={() => onGo({ kind: "skillFixes" })}
        writes={writes}
      />
      {!list ? (
        <main className="pane idle">
          <p className={got && !got.ok ? "error" : "muted"}>{got && !got.ok ? got.error : "Reading the skills…"}</p>
        </main>
      ) : skill ? (
        <SkillPane
          key={skill.id}
          skill={skill}
          list={list}
          tree={full}
          detail={detail}
          drafts={drafts}
          handle={pane}
          usage={usage}
          uses={uses}
          writes={writes}
          onBack={() => act({ kind: "back" })}
          onOpen={openSkill}
        />
      ) : (
        <SkillsOverview
          list={list}
          tree={full}
          usage={usage}
          uses={uses}
          note={note}
          writes={writes}
          tidied={tidied}
          onTidied={setTidied}
          onBack={fixes ? () => act({ kind: "back" }) : null}
          onOpen={openSkill}
        />
      )}
    </>
  );
}

function Chevron({ open }: { open: boolean }) {
  return (
    <svg className="chevron" aria-hidden="true" width="10" height="10" viewBox="0 0 10 10">
      <polyline points={open ? "1.5 3 5 7 8.5 3" : "3 1.5 7 5 3 8.5"} />
    </svg>
  );
}

export function SkillsSide({
  got,
  sections,
  open,
  cursor,
  openId,
  query,
  cli,
  filter,
  gets,
  used = null,
  onQuery,
  onFilterKey,
  onCli,
  onFold,
  onOpen,
  onFixes,
  writes = null,
}: {
  got: Read<Skills> | null;
  sections: Section[];
  open: ReadonlySet<string>;
  cursor: string | null;
  openId: string | null;
  query: string;
  cli: SkillCli | null;
  filter: RefObject<HTMLInputElement | null>;
  gets: (g: Group) => string | null;
  /// The quiet column of when each was used last; null for none.
  used?: Used | null;
  onQuery: (q: string) => void;
  onFilterKey: (e: KeyboardEvent<HTMLInputElement>) => void;
  onCli: (cli: SkillCli | null) => void;
  onFold: (key: string) => void;
  onOpen: (id: string, key: string) => void;
  onFixes: () => void;
  /// The plugins' switches write through these; none draws no switch.
  writes?: Writes | null;
}) {
  const nav = useRef<HTMLElement>(null);
  useEffect(() => {
    nav.current?.querySelector("[data-cursor]")?.scrollIntoView({ block: "nearest" });
  }, [cursor]);
  const list = got?.ok ? got.value : null;
  const chips: [SkillCli | null, string, number][] = list
    ? [
        [null, "All", list.skills.length],
        ...list.clis.map((c): [SkillCli, string, number] => [
          c,
          CLI_TITLE[c],
          list.skills.filter((s) => s.loads.some((l) => l.cli === c)).length,
        ]),
      ]
    : [];
  const empty = sections.every((sec) => sec.groups.length === 0);

  const group = (g: Group, depth: number) => {
    const unfolded = open.has(g.key);
    const problems = problemCount(g);
    const line = unfolded ? gets(g) : null;
    const control = writes && g.key.startsWith("plugin:") ? pluginControl(g) : null;
    const className = g.off ? "row group-row off" : "row group-row";
    const head = (
      <>
        <Chevron open={unfolded} />
        <span className="g-title">{g.title}</span>
        <span className="g-count">{skillCount(rowsOf(g))}</span>
        <span className="g-note">{g.note}</span>
        {problems > 0 && (
          <span className="g-problems">
            <Dot tone="red" />
            {problems}
            <span className="sr"> with a problem</span>
          </span>
        )}
      </>
    );
    const usedAt = used && <UsedAt at={groupUsed(g, used.uses)} now={used.now} />;
    const at = control && `plugin:${control.plugin}`;
    const said = writes?.said?.at === at ? writes.said : null;
    return (
      <div key={g.key}>
        {control && writes && at ? (
          <div className={`${className} has-switch`} data-depth={depth} data-cursor={g.key === cursor || undefined}>
            <button type="button" className="fold" aria-expanded={unfolded} onClick={() => onFold(g.key)}>
              {head}
            </button>
            {usedAt}
            <Toggle
              on={control.state === "on"}
              label={`${g.title} plugin, for Claude`}
              busy={writes.busy === at}
              onFlip={(on) =>
                writes.write(at, () => api.switchPlugin({ plugin: control.plugin!, on }), {
                  done: `${on ? "On" : "Off"} for new sessions; a running one needs /reload-plugins.`,
                })
              }
            />
          </div>
        ) : (
          <button
            type="button"
            className={className}
            data-depth={depth}
            data-cursor={g.key === cursor || undefined}
            aria-expanded={unfolded}
            onClick={() => onFold(g.key)}
          >
            {head}
            {usedAt}
          </button>
        )}
        {said && (
          <p className={said.error ? "gets said error" : "gets said"} data-depth={depth + 1} role={said.error ? "alert" : "status"}>
            <SaidText text={said.text} />
          </p>
        )}
        {unfolded && (
          <>
            {line && (
              <p className="gets" data-depth={depth + 1}>
                {line}
              </p>
            )}
            {g.children.map((c) => group(c, depth + 1))}
            {g.rows.map((r) => (
              <RouteLink
                key={r.key}
                to={{ kind: "skill", id: r.skill.id }}
                onGo={() => onOpen(r.skill.id, r.key)}
                className="row skill-row"
                data-depth={depth + 1}
                data-cursor={r.key === cursor || undefined}
                aria-current={r.skill.id === openId ? "page" : undefined}
              >
                <span className="title">{r.skill.name}</span>
                {r.cliNote && <span className="s-clis">{r.cliNote}</span>}
                {problemsOf(r.skill).length > 0 && (
                  <>
                    <Dot tone="red" />
                    <span className="sr">, {count(problemsOf(r.skill).length, "problem")}</span>
                  </>
                )}
                {used && <UsedAt at={rowUsed(r, used.uses)} now={used.now} />}
              </RouteLink>
            ))}
          </>
        )}
      </div>
    );
  };

  return (
    <div className="side skills-side">
      <div className="filter">
        <div className="filter-box">
          <input
            ref={filter}
            type="search"
            value={query}
            onChange={(e) => onQuery(e.target.value)}
            onKeyDown={onFilterKey}
            placeholder="Filter by name or what it is for"
            aria-label="Filter skills by name or what they are for"
            aria-keyshortcuts="/"
            autoComplete="off"
            spellCheck={false}
          />
          <kbd aria-hidden="true">/</kbd>
        </div>
        {chips.length > 2 && (
          <div className="chips" role="group" aria-label="Show what one CLI loads">
            {chips.map(([c, title, n]) => (
              <button key={c ?? "all"} type="button" aria-pressed={cli === c} onClick={() => onCli(c)}>
                {title} {n}
              </button>
            ))}
          </div>
        )}
      </div>
      {list && list.fixes.length > 0 && (
        <RouteLink to={{ kind: "skillFixes" }} onGo={onFixes} className="row fixes-row">
          <Dot tone="red" />
          <span className="title">
            {list.fixes.length === 1 ? "1 thing needs" : `${list.fixes.length} things need`} fixing
          </span>
          <span className="more" aria-hidden="true">
            ›
          </span>
        </RouteLink>
      )}
      <nav className="board skills-tree" aria-label="Skills" ref={nav}>
        {!got && <p className="side-note">Reading the skills…</p>}
        {got && !got.ok && <p className="side-note error">{got.error}</p>}
        {got?.ok && empty && (
          <p className="side-note">{query.trim() ? "No skill matches the filter." : "No skill is installed."}</p>
        )}
        {sections.map(
          (sec) =>
            sec.groups.length > 0 && (
              <section key={sec.key} aria-labelledby={`skills-${sec.key}`}>
                <div className="section-head">
                  <h2 id={`skills-${sec.key}`} className="caps">
                    {SECTION[sec.key].title} {skillCount(sec.groups.flatMap(rowsOf))}
                  </h2>
                  <span className="quiet">{SECTION[sec.key].note}</span>
                </div>
                {sec.groups.map((g, i) => (
                  <div key={g.key}>
                    {g.space !== null && g.space !== sec.groups[i - 1]?.space && <p className="space-label">{g.space}</p>}
                    {group(g, 0)}
                  </div>
                ))}
              </section>
            ),
        )}
      </nav>
      <p className="hints">j/k move · ←/→ fold · ↵ open · / filter · esc back</p>
    </div>
  );
}

/// The switch of the plugin `g` is, which its skills' Claude switch is.
function pluginControl(g: Group): SkillControl | null {
  for (const r of rowsOf(g)) {
    const c = controlOf(r.skill, "claude");
    if (c?.setting === "enabledPlugins") return c;
  }
  return null;
}

/// An on and off switch, the flip it is making drawn while it writes.
export function Toggle({ on, label, busy, onFlip }: { on: boolean; label: string; busy: boolean; onFlip: (on: boolean) => void }) {
  const shown = busy ? !on : on;
  return (
    <button
      type="button"
      role="switch"
      className="toggle"
      aria-checked={shown}
      aria-label={label}
      aria-busy={busy || undefined}
      onClick={() => busy || onFlip(!on)}
    >
      <span className="track" aria-hidden="true">
        <span className="knob" />
      </span>
    </button>
  );
}

/// A switch's positions side by side, one pressed: the one it is moving to
/// while it writes. The arrow keys move between them.
export function Positions({
  control: c,
  label,
  busy,
  onPick,
}: {
  control: SkillControl;
  label: string;
  busy: boolean;
  onPick: (to: SkillSetting) => void;
}) {
  const [picked, setPicked] = useState<SkillSetting | null>(null);
  const shown = busy && picked ? picked : c.state;
  const keys = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    const buttons = [...e.currentTarget.querySelectorAll("button")];
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    buttons[(at + (e.key === "ArrowRight" ? 1 : -1) + buttons.length) % buttons.length]?.focus();
  };
  return (
    <div role="group" aria-label={label} className="positions" aria-busy={busy || undefined} onKeyDown={keys}>
      {POSITIONS[c.setting].map((p) => (
        <button
          key={p}
          type="button"
          aria-pressed={shown === p}
          onClick={() => {
            if (busy || p === c.state) return;
            setPicked(p);
            onPick(p);
          }}
        >
          {SETTING_TITLE[p]}
        </button>
      ))}
    </div>
  );
}

/// A row's quiet last-used time, its column kept when there is none.
function UsedAt({ at, now }: { at: number | null; now: number }) {
  return (
    <span className="used">
      {at !== null && (
        <>
          <span className="sr">, used </span>
          {usedLabel(at, now)}
        </>
      )}
    </span>
  );
}

const PROBLEM_TITLE: Record<SkillProblemKind, string> = {
  brokenLink: "Broken link",
  linkOutside: "Links out",
  unreadable: "Unreadable",
  badFrontmatter: "Frontmatter",
  nameMismatch: "Name",
  badName: "Name",
  longDescription: "Description",
  missingField: "Missing field",
  duplicateName: "Same name",
  oneCli: "One CLI",
};

export function SkillsOverview({
  list,
  tree,
  usage = null,
  uses = NO_USES,
  note,
  writes = null,
  tidied = null,
  onTidied = () => {},
  onBack,
  onOpen,
}: {
  list: Skills;
  tree: Section[];
  /// How often Claude used each skill; null until it answers.
  usage?: Read<SkillUsage> | null;
  uses?: Uses;
  note: string | null;
  /// The fixes' and the tidy-up's buttons write through these; none draws
  /// no button.
  writes?: Writes | null;
  tidied?: Tidied | null;
  onTidied?: (t: Tidied | null) => void;
  /// On a screen of its own, the way back to the list.
  onBack: (() => void) | null;
  onOpen: (id: string) => void;
}) {
  const problems = list.skills.reduce((n, s) => n + problemsOf(s).length, 0);
  // Once its cure is written a fix is gone, so what it did is said above
  // them; an error stays on its row while the row is there.
  const said = writes?.said?.at.startsWith("fix:") ? writes.said : null;
  const fixed = said && (!said.error || !list.fixes.some((f) => said.at === `fix:${f.id}`)) ? said : null;
  const keeps = list.fixes.some((f) => f.acts.some((a) => a.act === "keep"));
  return (
    <main className="pane idle skills-overview">
      {onBack && (
        <div className="detail-top">
          <RouteLink to={{ kind: "skills" }} onGo={onBack} className="back">
            ‹ Skills
          </RouteLink>
        </div>
      )}
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      {fixed && writes && <DoneNote said={fixed} writes={writes} />}
      <div className="intro">
        <h1>Skills</h1>
        <p className="muted">{skillsSummary(tree, list.skills, list.clis, list.fixes.length)}</p>
      </div>
      {list.fixes.length > 0 ? (
        <section className="fixes" aria-labelledby="fixes-title">
          <div className="fixes-head">
            <h2 id="fixes-title" className="caps lane-needsYou">
              Needs fixing {list.fixes.length}
            </h2>
            <span className="quiet">
              {count(problems, "problem")}, one row per fix{keeps && " · Keep hides a row until one of its files changes"}
            </span>
          </div>
          <ul className="fix-list">
            {list.fixes.map((f) => (
              <FixRow key={f.id} fix={f} list={list} writes={writes} onOpen={onOpen} />
            ))}
          </ul>
          {list.kept.length > 0 && <KeptLine list={list} writes={writes} />}
        </section>
      ) : (
        <div className="fixes">
          <p className="calm">Nothing needs fixing.</p>
          {list.kept.length > 0 && <KeptLine list={list} writes={writes} />}
        </div>
      )}
      {list.clis.includes("claude") && (
        <UseSection
          list={list}
          tree={tree}
          usage={usage}
          uses={uses}
          writes={writes}
          tidied={tidied}
          onTidied={onTidied}
          onOpen={onOpen}
        />
      )}
    </main>
  );
}

const NO_USES: Uses = new Map();

/// How many of the most used skills show before the rest are asked for.
const MOST_SHOWN = 6;

/// USE, LAST 30 DAYS: the skills Claude used most, and in each place how
/// many of the skills it loads it did not use.
function UseSection({
  list,
  tree,
  usage,
  uses,
  writes,
  tidied,
  onTidied,
  onOpen,
}: {
  list: Skills;
  tree: Section[];
  usage: Read<SkillUsage> | null;
  uses: Uses;
  writes: Writes | null;
  tidied: Tidied | null;
  onTidied: (t: Tidied | null) => void;
  onOpen: (id: string) => void;
}) {
  const [all, setAll] = useState(false);
  const got = usage?.ok ? usage.value : null;
  const sum = useMemo(() => got && useSummary(tree, list.skills, list.clis, got, uses), [tree, list, got, uses]);
  const others = list.clis.filter((c) => c !== "claude").map((c) => CLI_TITLE[c]);
  const shown = sum && (all ? sum.most : sum.most.slice(0, MOST_SHOWN));
  return (
    <section className="use" aria-labelledby="use-title">
      <div className="use-top">
        <h2 id="use-title" className="caps">
          Use, last {got?.days ?? 30} days
        </h2>
        <span className="quiet">
          from Claude Code transcripts{others.length > 0 && `; ${and(others)} ${others.length === 1 ? "is" : "are"} not read`}
        </span>
      </div>
      {!usage ? (
        <p className="quiet">Reading the transcripts…</p>
      ) : !usage.ok ? (
        <p className="error">{usage.error}</p>
      ) : (
        sum &&
        shown && (
          <div className="use-cols">
            <div className="use-col">
              <h3 className="use-head">
                Most used · {count(sum.most.length, "skill")} in all
              </h3>
              {sum.most.length === 0 ? (
                <p className="quiet">None was used.</p>
              ) : (
                <ul className="use-list">
                  {shown.map((m) => (
                    <li key={m.skill.id}>
                      <RouteLink to={{ kind: "skill", id: m.skill.id }} onGo={() => onOpen(m.skill.id)} className="use-row">
                        <span className="u-name">{m.name}</span>
                        <span className="u-where">{m.where}</span>
                        <span className="u-count">{m.count}</span>
                      </RouteLink>
                    </li>
                  ))}
                </ul>
              )}
              {sum.most.length > MOST_SHOWN && (
                <button type="button" className="use-more" aria-expanded={all} onClick={() => setAll(!all)}>
                  {all ? `Only the top ${MOST_SHOWN}` : `All ${sum.most.length} by use ›`}
                </button>
              )}
            </div>
            <div className="use-col">
              <h3 className="use-head">Not used · {sum.unused} that Claude loads</h3>
              {sum.notUsed.length > 0 && (
                <ul className="use-list">
                  {sum.notUsed.map((n) => (
                    <li key={n.title} className="use-row">
                      <span className="u-name">{n.title}</span>
                      <span className="u-of">
                        {n.unused} of {n.of}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
              {sum.notCounted && <p className="quiet use-note">{sum.notCounted}</p>}
            </div>
          </div>
        )
      )}
      {got && writes && <TidyUp candidates={tidyCandidates(tree, uses)} days={got.days} writes={writes} tidied={tidied} onTidied={onTidied} />}
    </section>
  );
}

/// TIDY UP: one write that makes the skills of the user's own Claude loads
/// everywhere and did not use Slash menu only, naming every one first, and
/// one that puts each back as it was.
export function TidyUp({
  candidates,
  days,
  writes,
  tidied,
  onTidied,
}: {
  candidates: Skill[];
  days: number;
  writes: Writes;
  tidied: Tidied | null;
  onTidied: (t: Tidied | null) => void;
}) {
  if (!tidied && candidates.length === 0) return null;
  const said = writes.said?.at === "tidy" ? writes.said : null;
  const busy = writes.busy === "tidy";
  const n = tidied ? tidied.names.length : candidates.length;
  const them = n === 1 ? "it" : `the ${n}`;
  const tidy = () =>
    writes.write(
      "tidy",
      () => api.switchSkills({ cli: "claude", set: candidates.map((s) => ({ id: s.id, state: "userInvocableOnly" })) }),
      { then: (w) => onTidied({ names: candidates.map((s) => s.name), before: w.before }) },
    );
  const undo = () =>
    tidied &&
    writes.write(
      "tidy",
      () => api.switchSkills({ cli: "claude", set: tidied.before }),
      { done: `Put ${them} back as ${n === 1 ? "it was" : "they were"}.`, then: () => onTidied(null) },
    );
  return (
    <section className="tidy" aria-labelledby="tidy-title">
      <div className="use-top">
        <h2 id="tidy-title" className="caps">
          Tidy up
        </h2>
        <span className="quiet">Claude reads these descriptions in every session, and none was used in {days} days</span>
      </div>
      <div className="tidy-body">
        {!tidied ? (
          <>
            <h3 className="use-head">Yours, global · {n} not used</h3>
            <p className="tidy-names">{and(candidates.map((s) => s.name))}</p>
            <div>
              <button type="button" className="btn" aria-busy={busy || undefined} onClick={tidy}>
                Make {them} slash menu only
              </button>
            </div>
            <p className="quiet">Claude stops reading {n === 1 ? "its description" : "their descriptions"} from each session's next turn; /name still works.</p>
          </>
        ) : (
          <>
            <p>{`Made ${them} slash menu only: ${and(tidied.names)}.`}</p>
            <div>
              <button type="button" className="btn" aria-busy={busy || undefined} onClick={undo}>
                Undo
              </button>
            </div>
          </>
        )}
        {said && (
          <p className={said.error ? "error" : "said"} role={said.error ? "alert" : "status"}>
            <SaidText text={said.text} />
          </p>
        )}
      </div>
    </section>
  );
}

/// What a write said, its repository's few words and a slash command it
/// names each kept together on a line.
function SaidText({ text }: { text: string }) {
  const at = text.lastIndexOf(" · ");
  const head = at < 0 ? text : text.slice(0, at);
  const parts = head.split(/(?<= )(\/[a-z][a-z-]*)/);
  return (
    <>
      {parts.map((p, i) => (i % 2 === 1 ? <span key={i} className="together">{p}</span> : p))}
      {at >= 0 && (
        <>
          {" · "}
          <span className="together">{text.slice(at + 3)}</span>
        </>
      )}
    </>
  );
}

/// What an act on a fix did, above the fixes or under a skill's problems,
/// and its Undo, which calls `then` once it is undone, and with `reread`
/// reads the list again when it is refused.
export function DoneNote({ said, writes, then, reread }: { said: Said; writes: Writes; then?: () => void; reread?: boolean }) {
  const undo = said.undo;
  return (
    <div className={said.error ? "note done error" : "note done"} role={said.error ? "alert" : "status"}>
      <div className="done-text">
        <p>
          <SaidText text={said.text} />
        </p>
        {said.skipped?.map((t) => (
          <p key={t} className="skipped">
            <Inline text={t} />
          </p>
        ))}
      </div>
      {undo && (
        <button type="button" className="btn" aria-busy={writes.busy === said.at || undefined} onClick={() => writes.act(said.at, () => api.undo(undo), { then, reread })}>
          Undo
        </button>
      )}
    </div>
  );
}

/// `n kept · show` under what needs fixing, and the kept fixes, each with
/// a way back onto the list.
export function KeptLine({ list, writes, open: initial = false }: { list: Skills; writes: Writes | null; open?: boolean }) {
  const [open, setOpen] = useState(initial);
  return (
    <div className="kept">
      <p className="quiet">
        {list.kept.length} kept ·{" "}
        <button type="button" className="link" aria-expanded={open} onClick={() => setOpen(!open)}>
          {open ? "hide" : "show"}
        </button>
      </p>
      {open && (
        <ul className="kept-list">
          {list.kept.map((f) => (
            <li key={f.id}>
              <span className="k-title">
                {f.title}
                <span className="muted"> {f.place}</span>
              </span>
              {writes && (
                <button
                  type="button"
                  className="fix-act"
                  aria-busy={writes.busy === `fix:${f.id}` || undefined}
                  onClick={() => writes.act(`fix:${f.id}`, () => api.fix({ fix: f.id, act: "unkeep" }))}
                >
                  {actLabel(f, "unkeep")}
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

type Removal = "removeLinks" | "removeCommand";

const isRemoval = (act: SkillAct): act is Removal => act === "removeLinks" || act === "removeCommand";

export function FixRow({
  fix: f,
  list,
  writes,
  onOpen,
  comparing: initial = false,
}: {
  fix: SkillFix;
  list: Skills;
  writes: Writes | null;
  onOpen: (id: string) => void;
  comparing?: boolean;
}) {
  const [shown, setShown] = useState(false);
  const [comparing, setComparing] = useState(initial);
  const removeButton = useRef<HTMLButtonElement>(null);
  const at = `fix:${f.id}`;
  const cure = writes && cureOf(f, list.skills);
  const failed = writes?.said?.at === at && writes.said.error ? writes.said.text : null;
  const skills = f.skills.map((id) => list.skills.find((s) => s.id === id)).filter((s): s is Skill => s !== undefined);
  const one = skills.length === 1 ? skills[0]! : null;
  const named = new Set(skills.map((s) => s.name)).size === skills.length;
  const asked = writes?.asking?.startsWith(`${at}:`) ? (writes.asking.slice(at.length + 1) as SkillAct) : null;
  const asking = asked && isRemoval(asked) ? f.acts.find((a) => a.act === asked) ?? null : null;
  const command = skills.find((s) => s.kind === "command");
  const wasAsking = useRef(false);
  const cancelButton = useRef<HTMLButtonElement>(null);
  // The keys go to Cancel while it asks, and back to the button that asked.
  useEffect(() => {
    if (asking) cancelButton.current?.focus();
    else if (wasAsking.current) removeButton.current?.focus();
    wasAsking.current = asking !== null;
  }, [asking]);

  const buttons: { key: string; label: string; onClick: () => void; expanded?: boolean; busy: boolean; ref?: RefObject<HTMLButtonElement | null> }[] = [];
  if (cure && writes) {
    buttons.push({
      key: "cure",
      label: cure.label,
      busy: writes.busy === at,
      onClick: () => writes.write(at, () => api.switchSkills({ cli: cure.cli, set: [cure.to] }), { done: cure.done }),
    });
  }
  for (const { act } of writes ? f.acts.filter((a) => a.refused === null) : []) {
    if (!writes) break;
    if (isRemoval(act)) {
      buttons.push({ key: act, label: actLabel(f, act), busy: false, ref: removeButton, onClick: () => writes.ask(`${at}:${act}`) });
    } else if (act === "compare") {
      buttons.push({ key: act, label: comparing ? "Hide the copies" : actLabel(f, act), busy: false, expanded: comparing, onClick: () => setComparing(!comparing) });
    } else {
      buttons.push({ key: act, label: actLabel(f, act), busy: writes.busy === at, onClick: () => writes.act(at, () => api.fix({ fix: f.id, act })) });
    }
  }
  return (
    <li className="fix">
      <div className="fix-body">
        <p>
          <span className="fix-title">{f.title}</span>
          <span className="muted"> {f.place}</span>
        </p>
        <p className="fix-text">
          <Inline text={f.text} />
        </p>
        <p className="quiet">
          <Inline text={f.fix} />
        </p>
        {f.acts.map(
          (a) =>
            a.refused && (
              <p key={a.act} className="quiet refused">
                <Inline text={a.refused} />
              </p>
            ),
        )}
        {failed && (
          <p className="error" role="alert">
            {failed}
          </p>
        )}
        {asking ? (
          <div className="fix-confirm">
            <p>{removeAsk(f, asking.act as Removal, command ? tilde(command.loads[0]?.path ?? command.path, list.home) : null)}</p>
            {asking.says && <p className="quiet">{asking.says}</p>}
            {asking.act === "removeLinks" && (
              <ul className="fix-skills" aria-label="The links it removes">
                {skills.map((s) => (
                  <li key={s.id}>{s.path.slice(s.path.lastIndexOf("/") + 1)}</li>
                ))}
              </ul>
            )}
          </div>
        ) : (
          shown && (
            // Skills of one name are told apart by their paths; any others,
            // whose folder the fix names, by their names alone.
            <ul className={named ? "fix-skills" : "fix-skills paths"}>
              {skills.map((s) => (
                <li key={s.id}>
                  <RouteLink to={{ kind: "skill", id: s.id }} onGo={() => onOpen(s.id)}>
                    {s.name}
                  </RouteLink>
                  {!named && (
                    <span className="quiet">
                      <PathText path={tilde(s.path, list.home)} />
                    </span>
                  )}
                </li>
              ))}
            </ul>
          )
        )}
      </div>
      <div className="fix-acts">
        {asking && writes ? (
          <>
            <button
              type="button"
              className="btn danger"
              aria-busy={writes.busy === at || undefined}
              onClick={() => writes.act(at, () => api.fix({ fix: f.id, act: asking.act as Removal }))}
            >
              {actLabel(f, asking.act)}
            </button>
            <button ref={cancelButton} type="button" className="fix-act" onClick={() => writes.ask(null)}>
              Cancel
            </button>
          </>
        ) : (
          <>
            {buttons.map((b, i) => (
              <button
                key={b.key}
                ref={b.ref}
                type="button"
                className={i === 0 ? "btn" : "fix-act"}
                aria-expanded={b.expanded}
                aria-busy={b.busy || undefined}
                onClick={b.onClick}
              >
                {b.label}
              </button>
            ))}
            {one ? (
              <RouteLink to={{ kind: "skill", id: one.id }} onGo={() => onOpen(one.id)} className="fix-act">
                Open it ›
              </RouteLink>
            ) : (
              <button type="button" className="fix-act" aria-expanded={shown} onClick={() => setShown(!shown)}>
                {shown ? "Hide them" : "Show them"}
              </button>
            )}
          </>
        )}
      </div>
      {comparing && <Compare fix={f} list={list} />}
    </li>
  );
}

/// The two copies a fix is about, read when it opens.
function Compare({ fix, list }: { fix: SkillFix; list: Skills }) {
  return <CompareView got={useCompare(fix.id)} home={list.home} />;
}

/// Two SKILL.md files side by side, read-only, the lines that differ
/// marked; one line when they are the same.
export function CompareView({ got, home }: { got: Read<SkillCompare> | null; home: string }) {
  if (!got) return <p className="compare muted">Reading the two files…</p>;
  if (!got.ok) return <p className="compare error">{got.error}</p>;
  const files = got.value.files;
  const path = (f: (typeof files)[number]) => tilde(f.file ?? "", home);
  if (got.value.same) {
    return (
      <p className="compare same">
        The two are the same, byte for byte: <PathText path={path(files[0]!)} /> and <PathText path={path(files[1]!)} />.
      </p>
    );
  }
  const lines = files.map((f) => (f.text === null ? null : linesOf(f.text)));
  const marks = lines[0] && lines[1] ? differing(lines[0], lines[1]) : null;
  return (
    <div className="compare" role="group" aria-label="The two copies side by side">
      {files.map((f, i) => {
        const these = lines[i];
        const marked = marks?.[i as 0 | 1];
        return (
          <figure key={f.skill} className="cmp-file">
            <figcaption>
              {f.file ? <PathText path={path(f)} /> : "Not read"}
              {marked && <span className="muted"> · {count(marked.size, "line")} not in the other</span>}
            </figcaption>
            {these ? (
              <pre>
                {these.map((l, n) => (
                  <span key={n} className={marked?.has(n) ? "cmp-line differs" : "cmp-line"}>
                    {l}
                    {"\n"}
                  </span>
                ))}
              </pre>
            ) : (
              <p className="error">{f.unread}</p>
            )}
          </figure>
        );
      })}
    </div>
  );
}

/// A path that wraps after a `/` before it wraps anywhere else.
function PathText({ path }: { path: string }) {
  const parts = path.split("/");
  return (
    <>
      {parts.map((part, i) => (
        <span key={i}>
          {part}
          {i < parts.length - 1 && (
            <>
              /<wbr />
            </>
          )}
        </span>
      ))}
    </>
  );
}

/// A sentence the server wrote, its `code` spans drawn as code.
function Inline({ text }: { text: string }) {
  const html = useMemo(() => md.renderInline(text), [text]);
  return <span dangerouslySetInnerHTML={{ __html: html }} />;
}

type Tab = "file" | "frontmatter" | "files";

export function SkillPane({
  skill: s,
  list,
  tree,
  detail,
  drafts = new Map(),
  handle,
  usage,
  uses,
  writes,
  onBack,
  onOpen,
}: {
  skill: Skill;
  list: Skills;
  tree: Section[];
  detail: Read<SkillDetail> | null;
  drafts?: Map<string, Draft>;
  /// What the page's `e` calls.
  handle?: Ref<{ edit: () => void }>;
  usage: Read<SkillUsage> | null;
  uses: Uses;
  writes: Writes;
  onBack: () => void;
  onOpen: (id: string) => void;
}) {
  const [tab, setTab] = useState<Tab>("file");
  const [comparing, setComparing] = useState(false);
  const [editing, setEditing] = useState(() => drafts.has(s.id));
  // The detail a save or its Undo made old, shown as being read again
  // until the new one comes, so the old text never shows after a save.
  const [stale, setStale] = useState<Read<SkillDetail> | null>(null);
  const editButton = useRef<HTMLButtonElement>(null);
  const wasEditing = useRef(editing);
  const readAt = useMemo(() => Date.now(), [detail]);
  const home = list.home;
  // The fix of two skills of one name, or of one only one CLI loads, that
  // this one is in: the page can keep it, or compare the two.
  const fix = [...list.fixes, ...list.kept].find(
    (f) => f.skills.includes(s.id) && f.acts.some((a) => a.act === "keep" || a.act === "unkeep" || a.act === "compare"),
  );
  const fixAt = fix && `fix:${fix.id}`;
  const fixSaid = writes.said && writes.said.at === fixAt ? writes.said : null;
  const other = fix?.acts.some((a) => a.act === "compare") ? list.skills.find((x) => x.id !== s.id && fix.skills.includes(x.id)) : undefined;
  const at = placeOf(tree, s.id);
  const crumb = at ? [SECTION[at.section].title, ...at.path.map((g) => g.title)].join(" › ") : null;
  const d = detail?.ok ? detail.value : null;
  const editAt = `edit:${s.id}`;
  const editSaid = writes.said?.at === editAt ? writes.said : null;
  const fileName = d?.file ? d.file.slice(d.file.lastIndexOf("/") + 1) : "SKILL.md";
  const draft = drafts.get(s.id) ?? (d?.edit && d.file ? draftOf(d.file, d.edit, readAt) : null);
  const startEdit = () => {
    if (draft) setEditing(true);
  };
  useImperativeHandle(handle, () => ({ edit: startEdit }));
  // The keys go back to Edit once the editor closes.
  useEffect(() => {
    if (!editing && wasEditing.current) editButton.current?.focus();
    wasEditing.current = editing;
  }, [editing]);
  const tabs: [Tab, string][] = [
    ["file", s.kind === "command" ? "Command" : "SKILL.md"],
    ["frontmatter", d ? `Frontmatter ${d.frontmatter.length}` : "Frontmatter"],
    ["files", d ? `Files ${d.files.length}${d.truncated ? "+" : ""}` : "Files"],
  ];
  const loads = list.clis.map((c) => whereItLoads(s, c, list.skills, home));
  const about = controlsText(s);
  // A switch a control above shows is not said again under it.
  const others = s.switches.filter(
    (w) =>
      !s.controls.some(
        (c) => c.cli === w.cli && c.project === w.project && c.setting === w.setting && w.file !== null && tilde(c.file, home) === tilde(w.file, home),
      ),
  );
  const said = writes.said?.at.startsWith(`skill:${s.id}:`) ? writes.said : null;
  const pick = (c: SkillControl, to: SkillSetting) => {
    const at = `skill:${s.id}:${c.cli}`;
    if (c.setting === "enabledPlugins") {
      writes.write(at, () => api.switchPlugin({ plugin: c.plugin!, on: to === "on" }));
    } else {
      writes.write(at, () => api.switchSkills({ cli: c.cli, set: [{ id: s.id, state: to }] }));
    }
  };
  return (
    <main className="pane detail skill-detail">
      <div className="detail-top">
        <RouteLink to={{ kind: "skills" }} onGo={onBack} className="back">
          ‹ Skills
        </RouteLink>
        {crumb && <span className="meta">{crumb}</span>}
        {!editing &&
          (draft ? (
            <div className="actions">
              <button ref={editButton} type="button" className="btn" aria-keyshortcuts="e" onClick={startEdit}>
                Edit {fileName} <kbd>e</kbd>
              </button>
            </div>
          ) : (
            d?.uneditable && (
              <p className="edit-why quiet">
                <Inline text={d.uneditable} />
              </p>
            )
          ))}
      </div>

      <div className="title-block">
        <h1>{s.name}</h1>
        {s.description && <p className="why">{s.description}</p>}
      </div>

      {s.problems.length > 0 && (
        <section
          className={problemsOf(s).length === 0 ? "problems kept" : "problems"}
          aria-label={count(s.problems.length, "problem")}
        >
          {s.problems.map((p) => (
            <p key={p.text}>
              <span className="p-kind">{PROBLEM_TITLE[p.kind]}</span> · <Inline text={p.text} />
              {p.kept && <span className="muted"> Kept off what needs fixing until one of its files changes.</span>}
            </p>
          ))}
          {fix && fixAt && (
            <div className="fix-acts">
              {fix.acts.map((a) =>
                a.act === "compare" ? (
                  <button key={a.act} type="button" className="fix-act" aria-expanded={comparing} onClick={() => setComparing(!comparing)}>
                    {comparing ? "Hide the copies" : `Compare with ${other ? copyName(other, home) : "the other copy"}`}
                  </button>
                ) : a.act === "keep" || a.act === "unkeep" ? (
                  <button
                    key={a.act}
                    type="button"
                    className="btn"
                    aria-busy={writes.busy === fixAt || undefined}
                    onClick={() => writes.act(fixAt, () => api.fix({ fix: fix.id, act: a.act as "keep" | "unkeep" }))}
                  >
                    {actLabel(fix, a.act)}
                  </button>
                ) : null,
              )}
            </div>
          )}
          {fixSaid && <DoneNote said={fixSaid} writes={writes} />}
        </section>
      )}
      {comparing && fix && <Compare fix={fix} list={list} />}

      <dl className="facts">
        <dt>Path</dt>
        <dd>
          <PathText path={tilde(s.path, home)} />
        </dd>
        <dt>Repo</dt>
        <dd>
          {s.repo ? (
            <>
              <PathText path={tilde(s.repo.root, home)} />
              <span className="muted"> · {s.repo.branch ?? "detached HEAD"}</span>
              {d?.changes && (
                <>
                  <span className="muted"> · </span>
                  <span className="changes together">{d.changes}</span>
                </>
              )}
            </>
          ) : (
            <span className="muted">none</span>
          )}
        </dd>
        <dt>Source</dt>
        <dd>
          <Source skill={s} home={home} />
        </dd>
        {list.clis.includes("claude") && (
          <>
            <dt>Used</dt>
            <dd>
              <Use skill={s} usage={usage} uses={uses} />
            </dd>
          </>
        )}
      </dl>

      <section className="loaded" aria-labelledby="loads-title">
        <h2 id="loads-title" className="caps muted">
          Where it loads
        </h2>
        <div className="where">
          {loads.map((l) => {
            const c = l.where === null ? null : controlOf(s, l.cli);
            const failed = said?.error && said.at === `skill:${s.id}:${l.cli}` ? said.text : null;
            return (
              <div key={l.cli} className="where-row">
                <span className="w-cli">{CLI_TITLE[l.cli]}</span>
                <span className={l.where ? "w-where" : "w-where muted"}>{l.where ?? "no"}</span>
                <span className="w-path">
                  {l.where === null && l.why ? (
                    <Inline text={l.why} />
                  ) : (
                    l.paths.map((p) => (
                      <span key={p} className="w-line">
                        <PathText path={tilde(p, home)} />
                        {p !== s.path && " → link"}
                      </span>
                    ))
                  )}
                </span>
                <span className="w-state">
                  {l.instead ? (
                    <RouteLink to={{ kind: "skill", id: l.instead.id }} onGo={() => onOpen(l.instead!.id)}>
                      Open it ›
                    </RouteLink>
                  ) : c ? (
                    <Positions
                      control={c}
                      label={c.setting === "enabledPlugins" ? `The ${s.source.kind === "plugin" ? s.source.plugin : s.name} plugin, for ${CLI_TITLE[c.cli]}` : `${s.name}, for ${CLI_TITLE[c.cli]}`}
                      busy={writes.busy === `skill:${s.id}:${c.cli}`}
                      onPick={(to) => pick(c, to)}
                    />
                  ) : l.where === null ? null : l.state === null ? (
                    <span className="quiet">{CLI_TITLE[l.cli]} has no switch</span>
                  ) : (
                    l.state
                  )}
                </span>
                {failed && (
                  <p className="w-failed error" role="alert">
                    {failed}
                  </p>
                )}
              </div>
            );
          })}
        </div>
        {about && (
          <div className="w-about">
            <p className="quiet">
              <Inline text={about.writes} />
            </p>
            <p className="quiet">{about.when}</p>
            {said && !said.error && (
              <p className="said" role="status">
                <SaidText text={said.text} />
              </p>
            )}
          </div>
        )}
        {others.length > 0 && (
          <ul className="switches">
            {others.map((w) => (
              <li key={`${w.cli}:${w.setting}:${w.file}:${w.project}`}>
                {STATE_TITLE[w.state]} for {CLI_TITLE[w.cli]}
                {w.project && ` in ${w.project}`}: <Inline text={switchText(w, home)} />
              </li>
            ))}
          </ul>
        )}
      </section>

      {editing && draft ? (
        <SkillEditor
          key={s.id}
          skill={s}
          draft={draft}
          home={home}
          writes={writes}
          drafts={drafts}
          onClose={(saved) => {
            if (saved) setStale(detail);
            setEditing(false);
            setTab("file");
          }}
        />
      ) : (
        <>
          {editSaid && <DoneNote said={editSaid} writes={writes} then={() => setStale(detail)} reread />}
          <div className="tabs" role="tablist" aria-label="Files">
            {tabs.map(([id, label]) => (
              <button
                key={id}
                type="button"
                role="tab"
                id={`tab-${id}`}
                aria-selected={tab === id}
                aria-controls="skill-panel"
                onClick={() => setTab(id)}
              >
                {label}
              </button>
            ))}
          </div>
          <div id="skill-panel" role="tabpanel" aria-labelledby={`tab-${tab}`}>
            {!detail || detail === stale ? (
              <p className="muted">Reading the file…</p>
            ) : !detail.ok ? (
              <p className="error">{detail.error}</p>
            ) : tab === "file" ? (
              <FilePanel detail={detail.value} home={home} />
            ) : tab === "frontmatter" ? (
              <Frontmatter detail={detail.value} />
            ) : (
              <FilesPanel detail={detail.value} />
            )}
          </div>
        </>
      )}
    </main>
  );
}

/// What the editor holds besides the draft: what a check of the text
/// found, whether it asks before leaving, the file as a save found it
/// changed on disk, and what it shows.
export interface Editing {
  checked: (SkillCheck & { text: string }) | null;
  leaving: boolean;
  changed: { text: string; version: string; crlf: boolean; at: number } | null;
  showing: boolean;
  preview: boolean;
  error: string | null;
}

const SAVE_KEY = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.userAgent) ? "⌘S" : "Ctrl+S";

/// A skill's file in a textarea that grows with its text, checked by the
/// server as it is typed and saved by the skill's id and the version the
/// text was read at. Esc and Cancel leave at once when nothing changed,
/// and ask first when something did; what is typed and not saved stays in
/// `drafts` until it is.
export function SkillEditor({
  skill: s,
  draft: initial,
  home,
  writes,
  drafts,
  onClose,
  editing: seed = {},
}: {
  skill: Skill;
  draft: Draft;
  home: string;
  writes: Writes;
  drafts: Map<string, Draft>;
  /// Once saved, after the list is read again, or left.
  onClose: (saved: boolean) => void;
  editing?: Partial<Editing>;
}) {
  const [draft, setDraft] = useState(initial);
  const [checked, setChecked] = useState(seed.checked ?? null);
  const [leaving, setLeaving] = useState(seed.leaving ?? false);
  const [changed, setChanged] = useState(seed.changed ?? null);
  const [showing, setShowing] = useState(seed.showing ?? false);
  const [preview, setPreview] = useState(seed.preview ?? false);
  const [error, setError] = useState(seed.error ?? null);
  const area = useRef<HTMLTextAreaElement>(null);
  const keep = useRef<HTMLButtonElement>(null);
  const checks = useRef(0);
  const at = `edit:${s.id}`;
  const busy = writes.busy === at;
  const dirty = draft.text !== draft.read;
  const crlf = changed?.crlf ?? draft.crlf;
  const name = draft.file.slice(draft.file.lastIndexOf("/") + 1);
  const file = tilde(draft.file, home);
  const links = [...new Set(s.loads.filter((l) => l.path !== s.path).map((l) => tilde(l.path, home)))];
  const problems = checked?.problems ?? [];

  useEffect(() => area.current?.focus(), []);
  useEffect(() => {
    if (dirty) drafts.set(s.id, draft);
    else drafts.delete(s.id);
  }, [draft]);
  useEffect(() => {
    if (leaving) keep.current?.focus();
  }, [leaving]);

  /// What the server says of `text`, kept when it is the latest asked.
  const check = async (text: string) => {
    const mine = ++checks.current;
    const r = await api.check({ id: s.id, text: asSaved(text, crlf) });
    if (!r.ok) return null;
    const got = { ...r.value, text };
    if (mine === checks.current) setChecked(got);
    return got;
  };
  useEffect(() => {
    const text = draft.text;
    const timer = setTimeout(() => void check(text), checked ? 300 : 0);
    return () => clearTimeout(timer);
  }, [draft.text]);

  const close = (saved: boolean) => {
    drafts.delete(s.id);
    onClose(saved);
  };
  const leave = () => (leaveAction(dirty) === "ask" ? setLeaving(true) : close(false));
  const stay = () => {
    setLeaving(false);
    area.current?.focus();
  };
  const save = async () => {
    if (!dirty || writes.busy) return;
    const text = draft.text;
    // Problems not on screen when Save was pressed are shown first, and
    // Save then says how many: the next press saves with them.
    const seen = checked?.text === text;
    const got = seen ? checked : await check(text);
    if (!seen && got && got.problems.length > 0) return;
    const version = changed?.version ?? draft.version;
    // A refusal because the file moved on is said by what it moved on to.
    let moved = false;
    setError(null);
    writes.act(
      at,
      async () => {
        const r = await api.save({ id: s.id, version, text: asSaved(text, crlf) });
        if (r.ok) return r;
        const now = await api.detail(s.id);
        const disk = now.ok ? now.value.edit : null;
        if (disk && disk.version !== version) {
          moved = true;
          const read = asEdited(disk.text);
          setChanged({ text: read.text, version: disk.version, crlf: read.crlf, at: Date.now() });
        }
        return r;
      },
      {
        then: () => close(true),
        failed: (e) => setError(moved ? null : e),
      },
    );
  };
  const onKey = (e: KeyboardEvent<HTMLFormElement>) => {
    if (e.nativeEvent.isComposing) return;
    if ((e.metaKey || e.ctrlKey) && !e.altKey && e.key.toLowerCase() === "s") {
      e.preventDefault();
      void save();
    } else if (e.key === "Escape") {
      e.preventDefault();
      if (leaving) stay();
      else leave();
    }
  };

  return (
    <form
      className="editor"
      onKeyDown={onKey}
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <div className="editor-head">
        <label htmlFor="skill-text" className="caps">
          Editing {name}
        </label>
        <span className="muted">
          <PathText path={file} />
          {links.length > 0 && <>, the target of {and(links)}</>}
        </span>
      </div>
      {draft.note && (
        <p className="quiet">
          <Inline text={draft.note} />
        </p>
      )}
      {draft.mixed && <p className="quiet">Its line breaks are mixed, and saving makes every one of them LF.</p>}
      {preview && (
        <div className="reply skill-file">
          {!checked ? (
            <p className="muted">Reading the text…</p>
          ) : checked.body.trim() ? (
            <Markdown text={checked.body} />
          ) : (
            <p className="muted">Nothing after the frontmatter.</p>
          )}
        </div>
      )}
      <div className="grow" data-text={`${draft.text} `} hidden={preview}>
        <textarea
          id="skill-text"
          ref={area}
          value={draft.text}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          readOnly={busy}
          onChange={(e) => setDraft({ ...draft, text: e.target.value })}
        />
      </div>
      {problems.length > 0 && (
        <section className="problems" aria-label={`${count(problems.length, "problem")} in this text`}>
          <p className="muted">This text has {count(problems.length, "problem")}, which a save does not stop for:</p>
          {problems.map((p: SkillProblem) => (
            <p key={p.text}>
              <span className="p-kind">{PROBLEM_TITLE[p.kind]}</span> · <Inline text={p.text} />
            </p>
          ))}
        </section>
      )}
      {changed && (
        <div className="note edit-changed" role="alert">
          <p>
            <PathText path={file} /> changed on disk since the page read it, so your text was not saved. It is still here:{" "}
            <span className="together">Save over it</span> writes yours in its place, and its Undo puts the file back as it is now.
          </p>
          <button type="button" className="fix-act" aria-expanded={showing} onClick={() => setShowing(!showing)}>
            {showing ? "Hide the file" : "Show the file beside yours"}
          </button>
        </div>
      )}
      {changed && showing && (
        <CompareView
          got={{
            ok: true,
            value: {
              files: [
                { skill: s.id, file: "The file now", text: changed.text, unread: null },
                { skill: "", file: "Your text", text: draft.text, unread: null },
              ],
              same: changed.text === draft.text,
            },
          }}
          home={home}
        />
      )}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      {leaving && (
        <div className="fix-confirm">
          <p>Discard your changes to {name}? The file stays as it is.</p>
        </div>
      )}
      <div className="editor-acts">
        {leaving ? (
          <>
            <button type="button" className="btn danger" onClick={() => close(false)}>
              Discard changes
            </button>
            <button ref={keep} type="button" className="fix-act" onClick={stay}>
              Keep editing
            </button>
          </>
        ) : (
          <>
            <button type="submit" className="btn primary" disabled={!dirty} aria-busy={busy || undefined} aria-keyshortcuts="Meta+S Control+S">
              {saveLabel(problems.length, changed !== null)} <kbd>{SAVE_KEY}</kbd>
            </button>
            <button type="button" className="btn" onClick={leave}>
              Cancel
            </button>
            <button type="button" className="fix-act" onClick={() => setPreview(!preview)}>
              {preview ? "Back to the text" : "Preview"}
            </button>
            <span className="quiet">
              {changed
                ? `Read again at ${clock(changed.at)}. Save over it refuses if the file changes again.`
                : `Read at ${clock(draft.at)}. Save refuses if the file changed since.`}
            </span>
          </>
        )}
      </div>
    </form>
  );
}

function Use({ skill, usage, uses }: { skill: Skill; usage: Read<SkillUsage> | null; uses: Uses }) {
  if (!usage) return <span className="muted">reading the transcripts…</span>;
  if (!usage.ok) return <span className="error">{usage.error}</span>;
  const { text, quiet } = usedText(skill, usage.value, uses);
  return quiet ? <span className="muted">{text}</span> : <>{text}</>;
}

function Source({ skill: s, home }: { skill: Skill; home: string }) {
  const src = s.source;
  const links = [
    ...new Set(s.loads.filter((l) => l.path !== s.path).map((l) => tilde(l.path.slice(0, l.path.lastIndexOf("/")), home))),
  ];
  const linked = links.length > 0 && <>, linked into {and(links)}</>;
  switch (src.kind) {
    case "repo":
      return (
        <>
          A folder in the {s.repo ? s.repo.root.slice(s.repo.root.lastIndexOf("/") + 1) : ""} repository{linked}.
        </>
      );
    case "folder":
      return <>A folder of its own, in no repository{linked}.</>;
    case "installer":
      return (
        <>
          Installed by <code>npx skills</code> from{" "}
          <External href={`https://github.com/${src.source}`}>{src.source} ↗</External>
          {src.path && <span className="muted"> · {src.path}</span>}
        </>
      );
    case "plugin":
      return (
        <>
          The <code>{src.plugin}</code> plugin{src.version && <span className="muted"> {src.version}</span>}, from the{" "}
          <code>{src.marketplace}</code> marketplace
          {src.from && (
            <>
              {" "}
              (
              {src.from.startsWith("github.com/") ? (
                <External href={`https://${src.from}`}>{src.from} ↗</External>
              ) : (
                tilde(src.from, home)
              )}
              )
            </>
          )}
          .
        </>
      );
    case "claudeAi":
      return <>Synced from the claude.ai account, which is where it changes.</>;
    case "bundled":
      return <>Bundled with {CLI_TITLE[src.cli]}.</>;
  }
}

function Frontmatter({ detail: d }: { detail: SkillDetail }) {
  if (d.frontmatterError) return <p className="error">{d.frontmatterError}</p>;
  if (d.frontmatter.length === 0) return <p className="muted">{d.body === null ? "Not read." : "None."}</p>;
  return (
    <table className="fields">
      <tbody>
        {d.frontmatter.map((f) => (
          <tr key={f.key}>
            <th scope="row">{f.key}</th>
            <td>{valueText(f.value)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function FilePanel({ detail: d, home }: { detail: SkillDetail; home: string }) {
  if (d.body === null) return <p className="error">{d.unread}</p>;
  return (
    <div className="reply skill-file">
      <div className="reply-meta">{d.file && <PathText path={tilde(d.file, home)} />}</div>
      {d.body.trim() ? <Markdown text={d.body} /> : <p className="muted">Nothing after the frontmatter.</p>}
    </div>
  );
}

function size(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function FilesPanel({ detail: d }: { detail: SkillDetail }) {
  if (d.files.length === 0) return <p className="muted">{d.unread ?? "No files."}</p>;
  return (
    <>
      <ul className="files">
        {d.files.map((f) => (
          <li key={f.path}>
            <span className="f-path">{f.path}</span>
            {f.link !== null ? (
              <span className="f-link muted">→ {f.link}</span>
            ) : (
              <span className="f-size">{size(f.size)}</span>
            )}
          </li>
        ))}
      </ul>
      {d.truncated && <p className="muted">And more, not listed.</p>}
    </>
  );
}
