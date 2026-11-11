import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type RefObject } from "react";
import { useSkillDetail, useSkills, type Read } from "./api/client";
import type { Skill, SkillCli, SkillDetail, SkillProblemKind, Skills } from "./api/contract";
import { moveCursor, type Route } from "./model";
import {
  bestMatch,
  CLI_SHORT,
  CLI_TITLE,
  clisOf,
  count,
  filterSkills,
  groupOf,
  groups,
  offFor,
  skillKey,
  skillsSummary,
  skillToOpen,
  STATE_TITLE,
  switchText,
  tilde,
  valueText,
  whereOf,
  type Group,
  type SkillAction,
} from "./skills";
import { Dot, External, Markdown, md, RouteLink } from "./ui";

function isTextField(t: HTMLElement): boolean {
  return t.isContentEditable || t.matches("input, textarea, select");
}

/// The Skills page: the list of every skill on the left, grouped by where
/// it comes from, and the open one (or an overview of what is wrong) on
/// the right. Every key does what a click on the same thing does.
export function SkillsPage({ openId, onGo }: { openId: string | null; onGo: (to: Route, replace?: boolean) => void }) {
  const got = useSkills();
  const list = got?.ok ? got.value : null;
  const home = list?.home ?? "";
  const [query, setQuery] = useState("");
  const [cursor, setCursor] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const filter = useRef<HTMLInputElement>(null);
  const shown = useMemo(() => (list ? groups(filterSkills(list.skills, query, home), home) : []), [list, query, home]);
  const items = useMemo(() => shown.flatMap((g) => g.skills), [shown]);
  const skill = list?.skills.find((s) => s.id === openId) ?? null;
  const detail = useSkillDetail(skill?.id ?? null, list);

  useEffect(() => {
    if (list && openId && !skill) {
      onGo({ kind: "skills" }, true);
      setNote(`No skill with id ${openId}.`);
    }
  }, [list, openId, skill, onGo]);

  useEffect(() => {
    if (openId) {
      setCursor(openId);
      setNote(null);
    }
  }, [openId]);

  useEffect(() => {
    document.title = skill ? `${skill.name} · Skills · nebula` : "Skills · nebula";
  }, [skill?.name]);

  const open = (id: string) => {
    setCursor(id);
    onGo({ kind: "skill", id });
  };
  /// What every key on the page does, and what the pointer does through the
  /// same calls.
  const act = (action: SkillAction) => {
    switch (action.kind) {
      case "move": {
        const at = items.findIndex((s) => s.id === cursor);
        const next = moveCursor(at < 0 ? null : at, action.delta, items.length);
        return setCursor(next === null ? null : items[next]!.id);
      }
      case "open": {
        const s = skillToOpen(items, cursor, query);
        return s && open(s.id);
      }
      case "back":
        return openId ? onGo({ kind: "skills" }) : setCursor(null);
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

  return (
    <>
      <SkillsSide
        got={got}
        groups={shown}
        clis={list?.clis ?? []}
        cursor={cursor}
        openId={openId}
        query={query}
        filter={filter}
        onQuery={(q) => {
          setQuery(q);
          const left = list ? groups(filterSkills(list.skills, q, home), home).flatMap((g) => g.skills) : [];
          setCursor(bestMatch(left, q)?.id ?? null);
        }}
        onFilterKey={filterKey}
        onOpen={open}
      />
      {!list ? (
        <main className="pane idle">
          <p className={got && !got.ok ? "error" : "muted"}>{got && !got.ok ? got.error : "Reading the skills…"}</p>
        </main>
      ) : skill ? (
        <SkillPane key={skill.id} skill={skill} list={list} detail={detail} onBack={() => act({ kind: "back" })} />
      ) : (
        <SkillsOverview list={list} note={note} onOpen={open} />
      )}
    </>
  );
}

export function SkillsSide({
  got,
  groups,
  clis,
  cursor,
  openId,
  query,
  filter,
  onQuery,
  onFilterKey,
  onOpen,
}: {
  got: Read<Skills> | null;
  groups: Group[];
  clis: SkillCli[];
  cursor: string | null;
  openId: string | null;
  query: string;
  filter: RefObject<HTMLInputElement | null>;
  onQuery: (q: string) => void;
  onFilterKey: (e: KeyboardEvent<HTMLInputElement>) => void;
  onOpen: (id: string) => void;
}) {
  const nav = useRef<HTMLElement>(null);
  useEffect(() => {
    nav.current?.querySelector("[data-cursor]")?.scrollIntoView({ block: "nearest" });
  }, [cursor]);
  return (
    <div className="side">
      <div className="filter">
        <input
          ref={filter}
          type="search"
          value={query}
          onChange={(e) => onQuery(e.target.value)}
          onKeyDown={onFilterKey}
          placeholder="Filter skills"
          aria-label="Filter skills"
          aria-keyshortcuts="/"
          autoComplete="off"
          spellCheck={false}
        />
        <kbd aria-hidden="true">/</kbd>
      </div>
      <nav className="board" aria-label="Skills" ref={nav}>
        {!got && <p className="side-note">Reading the skills…</p>}
        {got && !got.ok && <p className="side-note error">{got.error}</p>}
        {got?.ok && groups.length === 0 && (
          <p className="side-note">{query.trim() ? "No skill matches the filter." : "No skill is installed."}</p>
        )}
        {groups.map((g, i) => (
          <section key={g.key} aria-labelledby={`group-${i}`}>
            <div className="group-head">
              <h2 id={`group-${i}`} className="caps group-title">
                {g.title} <span className="group-count">{g.skills.length}</span>
              </h2>
              <span className="group-note" title={g.note}>
                {g.note}
              </span>
            </div>
            {g.skills.map((s) => (
              <RouteLink
                key={s.id}
                to={{ kind: "skill", id: s.id }}
                onGo={() => onOpen(s.id)}
                className="row skill-row"
                data-cursor={s.id === cursor || undefined}
                aria-current={s.id === openId ? "page" : undefined}
              >
                <SkillRow s={s} clis={clis} />
              </RouteLink>
            ))}
          </section>
        ))}
      </nav>
      <p className="hints">j/k move · ↵ open · / filter · esc back</p>
    </div>
  );
}

function SkillRow({ s, clis }: { s: Skill; clis: SkillCli[] }) {
  const loaded = new Set(clisOf(s));
  const label = clisOf(s)
    .map((c) => `${CLI_TITLE[c]}${offFor(s, c) ? " (off)" : ""}`)
    .join(", ");
  return (
    <>
      <span className="title">{s.name}</span>
      <span className="desc">{s.description ?? ""}</span>
      <span className="clis" aria-label={label ? `Loaded by ${label}` : "Loaded by no CLI"}>
        {clis.map((c) => (
          <span
            key={c}
            aria-hidden="true"
            className={!loaded.has(c) ? "cli none" : offFor(s, c) ? "cli off" : "cli"}
            title={loaded.has(c) ? `${CLI_TITLE[c]}${offFor(s, c) ? ", switched off" : ""}` : undefined}
          >
            {loaded.has(c) ? CLI_SHORT[c] : "·"}
          </span>
        ))}
      </span>
      {s.problems.length > 0 ? (
        <>
          <Dot tone="red" />
          <span className="sr">, {count(s.problems.length, "problem")}</span>
        </>
      ) : (
        <span className="dot-gap" aria-hidden="true" />
      )}
    </>
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

function SkillsOverview({ list, note, onOpen }: { list: Skills; note: string | null; onOpen: (id: string) => void }) {
  const places = new Set(list.skills.map((s) => groupOf(s, list.home).key)).size;
  const troubled = list.skills.filter((s) => s.problems.length > 0);
  return (
    <main className="pane idle">
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      <div className="intro">
        <h1>Skills</h1>
        <p className="muted">{skillsSummary(list.skills, list.clis, places)}</p>
      </div>
      {troubled.length > 0 ? (
        <section className="overview" aria-labelledby="overview-title">
          <h2 id="overview-title" className="caps lane-needsYou">
            Problems {troubled.length}
          </h2>
          <ul className="overview-list">
            {troubled.map((s) => (
              <li key={s.id}>
                <RouteLink to={{ kind: "skill", id: s.id }} onGo={() => onOpen(s.id)} className="overview-row">
                  <span className="o-name">{s.name}</span>
                  <span className="o-kind">{PROBLEM_TITLE[s.problems[0]!.kind]}</span>
                  <span className="o-text" title={s.problems.map((p) => p.text.replaceAll("`", "")).join("\n")}>
                    <Inline text={s.problems[0]!.text} />
                    {s.problems.length > 1 && <span className="muted"> · {s.problems.length - 1} more</span>}
                  </span>
                </RouteLink>
              </li>
            ))}
          </ul>
        </section>
      ) : (
        <p className="calm">Nothing is wrong with any skill.</p>
      )}
    </main>
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

type Tab = "file" | "files";

function SkillPane({
  skill: s,
  list,
  detail,
  onBack,
}: {
  skill: Skill;
  list: Skills;
  detail: Read<SkillDetail> | null;
  onBack: () => void;
}) {
  const [tab, setTab] = useState<Tab>("file");
  const home = list.home;
  const group = groupOf(s, home);
  const d = detail?.ok ? detail.value : null;
  const tabs: [Tab, string][] = [
    ["file", s.kind === "command" ? "Command" : "SKILL.md"],
    ["files", d ? `Files ${d.files.length}${d.truncated ? "+" : ""}` : "Files"],
  ];
  return (
    <main className="pane detail skill-detail">
      <div className="detail-top">
        <RouteLink to={{ kind: "skills" }} onGo={onBack} className="back">
          ‹ Skills
        </RouteLink>
        <span className="meta">
          {group.title}
          <span className="sep"> · </span>
          {s.kind === "command" ? "legacy command" : "skill"}
        </span>
      </div>

      <div className="title-block">
        <h1>{s.name}</h1>
        {s.description && (
          <p className="why" title={s.description}>
            {s.description}
          </p>
        )}
      </div>

      {s.problems.length > 0 && (
        <section className="problems" aria-labelledby="problems-title">
          <h2 id="problems-title" className="caps lane-needsYou">
            {s.problems.length === 1 ? "Problem" : `Problems ${s.problems.length}`}
          </h2>
          <ul>
            {s.problems.map((p) => (
              <li key={p.text}>
                <span className="p-kind">{PROBLEM_TITLE[p.kind]}</span>
                <span className="p-text">
                  <Inline text={p.text} />
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}

      <section className="linked facts" aria-label="Where it is">
        <dl>
          <div>
            <dt>Path</dt>
            <dd>
              <span className="wrap">
                <PathText path={tilde(s.path, home)} />
              </span>
            </dd>
          </div>
          <div>
            <dt>Repo</dt>
            <dd>
              {s.repo ? (
                <span className="wrap">
                  <PathText path={tilde(s.repo.root, home)} />
                  <span className="muted"> · {s.repo.branch ?? "detached HEAD"}</span>
                </span>
              ) : (
                <span className="muted">none</span>
              )}
            </dd>
          </div>
          <div>
            <dt>Source</dt>
            <dd>
              <span className="wrap">
                <Source skill={s} home={home} />
              </span>
            </dd>
          </div>
        </dl>
      </section>

      <section className="loaded" aria-labelledby="loads-title">
        <h2 id="loads-title" className="caps muted">
          {s.problems.some((p) => p.kind === "brokenLink") ? "Found by" : "Loaded by"}
        </h2>
        <ul className="loads">
          {s.loads.map((l, i) => (
            <li key={`${l.cli}:${l.path}:${i}`}>
              <span className="l-cli">{CLI_TITLE[l.cli]}</span>
              <span className="l-where">{whereOf(l)}</span>
              <code className="l-name">{l.name}</code>
              <span className="l-path">
                <PathText path={tilde(l.path, home)} />
                {l.path !== s.path && <span className="muted"> → link</span>}
              </span>
            </li>
          ))}
        </ul>
        {s.switches.length > 0 && (
          <ul className="switches">
            {s.switches.map((w) => (
              <li key={`${w.cli}:${w.setting}:${w.file}`}>
                <span className={w.state === "off" ? "state off" : "state"}>{STATE_TITLE[w.state]}</span>
                <span>
                  for {CLI_TITLE[w.cli]}
                  {w.project && ` in ${w.project}`}: <Inline text={switchText(w, home)} />
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="frontmatter" aria-labelledby="fm-title">
        <h2 id="fm-title" className="caps muted">
          Frontmatter
        </h2>
        <Frontmatter detail={detail} />
      </section>

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
        {!detail ? (
          <p className="muted">Reading the file…</p>
        ) : !detail.ok ? (
          <p className="error">{detail.error}</p>
        ) : tab === "file" ? (
          <FilePanel detail={detail.value} home={home} />
        ) : (
          <FilesPanel detail={detail.value} />
        )}
      </div>
    </main>
  );
}

function Source({ skill: s, home }: { skill: Skill; home: string }) {
  const src = s.source;
  switch (src.kind) {
    case "repo":
      return <>A folder in the {s.repo ? s.repo.root.slice(s.repo.root.lastIndexOf("/") + 1) : ""} repository.</>;
    case "folder":
      return <>A folder of its own, in no repository.</>;
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

function Frontmatter({ detail }: { detail: Read<SkillDetail> | null }) {
  if (!detail) return <p className="muted">Reading the file…</p>;
  if (!detail.ok) return null;
  const d = detail.value;
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
