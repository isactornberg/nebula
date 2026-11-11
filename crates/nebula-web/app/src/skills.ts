import type { FrontmatterValue, Project, Skill, SkillCli, SkillLoad, SkillScope, SkillSwitch, SkillUsage, SkillUse } from "./api/contract";
import { keyAction } from "./model";

/// `path` under `~` when it is in `home`.
export function tilde(path: string, home: string): string {
  if (!home) return path;
  if (path === home) return "~";
  return path.startsWith(`${home}/`) ? `~${path.slice(home.length)}` : path;
}

const base = (path: string) => path.slice(path.lastIndexOf("/") + 1);
const parent = (path: string) => path.slice(0, Math.max(path.lastIndexOf("/"), 1));

export const CLI_TITLE: Record<SkillCli, string> = { claude: "Claude", codex: "Codex", cursor: "Cursor" };
const CLIS: SkillCli[] = ["claude", "codex", "cursor"];

/// `items` as a list in a sentence: `a`, `a and b`, `a, b and c`.
export function and(items: string[]): string {
  return items.length > 1 ? `${items.slice(0, -1).join(", ")} and ${items.at(-1)}` : (items[0] ?? "");
}

/// A set of CLIs in a few words: `Claude only`, `Codex and Cursor`.
function cliText(clis: SkillCli[]): string {
  const names = clis.map((c) => CLI_TITLE[c]);
  return names.length === 1 ? `${names[0]} only` : and(names);
}

const GLOBAL_SCOPES: ReadonlySet<SkillScope> = new Set<SkillScope>([
  "managed",
  "personal",
  "plugin",
  "claudeAi",
  "admin",
  "system",
]);

/// A load every session gets, whatever its project.
const isGlobal = (l: SkillLoad) => GLOBAL_SCOPES.has(l.scope);

/// A space's name as projects are filed under it: without case, runs of
/// blanks as one.
const spaceKey = (space: string) => space.trim().replace(/\s+/g, " ").toLowerCase();

/// One skill where the list shows it, and the CLIs that load it there.
export interface Row {
  key: string;
  skill: Skill;
  clis: SkillCli[];
  /// The spaces whose projects also get it, through an added directory.
  spaces: string[];
  /// Its CLIs in words, when they are not its group's.
  cliNote: string | null;
}

/// A fold of the list: a place skills come from, or a project they load in.
export interface Group {
  key: string;
  title: string;
  /// Where it is, in a few words: a repository's path and branch, a
  /// plugin's version.
  note: string;
  rows: Row[];
  /// The plugins, under Plugins.
  children: Group[];
  /// A plugin switched off.
  off: boolean;
  /// For a project's group, the project.
  project: string | null;
  /// The space a project's group is filed under.
  space: string | null;
}

export interface Section {
  key: "global" | "projects";
  groups: Group[];
}

/// Every row of `g`, its plugins' included.
export const rowsOf = (g: Group): Row[] => [...g.rows, ...g.children.flatMap(rowsOf)];

/// How many skills `rows` hold, each once.
export const skillCount = (rows: Row[]) => new Set(rows.map((r) => r.skill.id)).size;

/// How many skills in `g` have a problem.
export const problemCount = (g: Group) => skillCount(rowsOf(g).filter((r) => r.skill.problems.length > 0));

/// The marketplace a plugin comes from when nothing says otherwise.
const OFFICIAL_MARKETPLACE = "claude-plugins-official";

type Meta = Omit<Group, "rows" | "children" | "off" | "project" | "space"> & { rank: number };

/// The place a skill every session gets comes from, as the list folds it.
function globalGroupOf(s: Skill, home: string): Meta & { parent?: Meta } {
  const src = s.source;
  switch (src.kind) {
    case "repo": {
      const root = s.repo?.root ?? parent(s.path);
      const branch = s.repo?.branch;
      return { key: `repo:${root}`, title: base(root), note: branch ? `${tilde(root, home)} · ${branch}` : tilde(root, home), rank: 0 };
    }
    case "folder":
      return { key: "folder", title: "Plain folders", note: "in no repository", rank: 1 };
    case "installer":
      return { key: "installer", title: "npx skills", note: "", rank: 2 };
    case "plugin": {
      const version = src.version && /^[0-9a-f]{12,}$/.test(src.version) ? src.version.slice(0, 7) : src.version;
      const market = src.marketplace.includes(src.plugin) || src.marketplace === OFFICIAL_MARKETPLACE ? null : src.marketplace;
      return {
        key: `plugin:${src.plugin}@${src.marketplace}`,
        title: src.plugin,
        note: [market, version].filter(Boolean).join(" · "),
        rank: 0,
        parent: { key: "plugins", title: "Plugins", note: "", rank: 3 },
      };
    }
    case "claudeAi":
      return { key: "claudeAi", title: "claude.ai", note: "synced from your account", rank: 4 };
    case "bundled":
      return { key: "bundled", title: "Built in", note: "", rank: 5 };
  }
}

/// The list as a tree: GLOBAL, what every session gets, folded by where
/// it comes from, then PROJECTS, what a session gets in one project. A
/// skill is in each place it loads for a CLI that does not load it
/// everywhere already, so a skill Claude loads everywhere and Codex in one
/// project is in both, under each with its own CLIs. With `cli`, only what
/// that CLI loads. `projects` are the daemon's, for their spaces and order.
export function skillTree(skills: Skill[], home: string, projects: Project[], cli: SkillCli | null = null): Section[] {
  const groups = new Map<string, Group & { rank: number }>();
  const groupFor = (meta: Meta, extra: Partial<Group> = {}) => {
    let g = groups.get(meta.key);
    if (!g) {
      const { rank, ...rest } = meta;
      g = { ...rest, rank, rows: [], children: [], off: false, project: null, space: null, ...extra };
      groups.set(meta.key, g);
    }
    return g;
  };
  const place = (g: Group, s: Skill, clis: SkillCli[], spaces: string[] = []) =>
    g.rows.push({ key: `${g.key}/${s.id}`, skill: s, clis, spaces, cliNote: null });
  const spaceOf = (project: string) => projects.find((p) => p.name === project)?.space ?? null;

  for (const s of skills) {
    const loads = cli ? s.loads.filter((l) => l.cli === cli) : s.loads;
    if (loads.length === 0) continue;
    const everywhere = clisIn(loads.filter(isGlobal));
    if (everywhere.length > 0) {
      const meta = globalGroupOf(s, home);
      const g = groupFor(meta);
      if (meta.parent) {
        const top = groupFor(meta.parent);
        if (!top.children.includes(g)) top.children.push(g);
      }
      place(g, s, everywhere);
    }
    const local = new Map<string, Set<SkillCli>>();
    for (const l of loads) {
      if ((l.scope !== "project" && l.scope !== "nested") || !l.project) continue;
      const has = local.get(l.project) ?? new Set();
      local.set(l.project, has.add(l.cli));
    }
    const added = loads.filter((l) => l.scope === "addedDir" && l.space);
    const spaces = [...new Set(added.map((l) => l.space!))];
    const addedClis = new Set(added.map((l) => l.cli));
    const notEverywhere = (has: Set<SkillCli>) => CLIS.filter((c) => has.has(c) && !everywhere.includes(c));
    // An added directory's skill is filed under the project it is in, and
    // under its space when it is in none.
    for (const [project, has] of local) {
      const clis = notEverywhere(new Set([...has, ...addedClis]));
      const space = spaceOf(project);
      if (clis.length > 0) place(groupFor({ key: `project:${project}`, title: project, note: "", rank: 10 }, { project, space }), s, clis, spaces);
    }
    if (local.size === 0) {
      for (const space of spaces) {
        const clis = notEverywhere(addedClis);
        const meta = { key: `space:${spaceKey(space)}`, title: "Added directories", note: "all its projects", rank: 11 };
        if (clis.length > 0) place(groupFor(meta, { space }), s, clis);
      }
    }
  }

  // Copies of one name: the one Claude loads first, then Codex's, then Cursor's.
  const byName = (a: Row, b: Row) =>
    a.skill.name.toLowerCase().localeCompare(b.skill.name.toLowerCase()) ||
    CLIS.indexOf(a.clis[0]!) - CLIS.indexOf(b.clis[0]!) ||
    a.skill.path.localeCompare(b.skill.path);
  for (const g of groups.values()) {
    g.rows.sort(byName);
    const sets = g.rows.map((r) => r.clis.join(" "));
    const usual = mostCommon(sets);
    for (const r of g.rows) r.cliNote = r.clis.join(" ") === usual ? null : cliText(r.clis);
    if (g.key.startsWith("plugin:")) {
      g.off = g.rows.every((r) => offFor(r.skill, "claude"));
      if (g.off) g.note = [g.note, "off"].filter(Boolean).join(" · ");
    }
  }
  for (const g of groups.values()) g.note = noteOf(g);

  const order = (a: Group & { rank: number }, b: Group & { rank: number }) => a.rank - b.rank || a.title.localeCompare(b.title);
  const tops = [...groups.values()].filter((g) => !g.key.startsWith("plugin:"));
  const global = tops.filter((g) => g.rank < 10).sort(order);
  for (const g of global) g.children.sort((a, b) => a.title.localeCompare(b.title));
  const at = (g: Group) => {
    const i = projects.findIndex((p) => p.name === g.project);
    return i < 0 ? projects.length : i;
  };
  const local = tops
    .filter((g) => g.rank >= 10)
    .sort(
      (a, b) =>
        Number(a.space !== null) - Number(b.space !== null) ||
        spaceKey(a.space ?? "").localeCompare(spaceKey(b.space ?? "")) ||
        a.rank - b.rank ||
        at(a) - at(b) ||
        a.title.localeCompare(b.title),
    );
  return [
    { key: "global", groups: global },
    { key: "projects", groups: local },
  ];
}

/// The CLIs `loads` are for, in a fixed order.
function clisIn(loads: SkillLoad[]): SkillCli[] {
  const has = new Set(loads.map((l) => l.cli));
  return CLIS.filter((c) => has.has(c));
}

/// The value `values` hold most often, the first of equals.
function mostCommon(values: string[]): string | undefined {
  const n = new Map<string, number>();
  for (const v of values) n.set(v, (n.get(v) ?? 0) + 1);
  return [...n.entries()].reduce<[string, number] | undefined>((best, e) => (best && best[1] >= e[1] ? best : e), undefined)?.[0];
}

/// A group's where-line, once its rows are in.
function noteOf(g: Group): string {
  const rows = rowsOf(g);
  switch (g.key) {
    case "installer": {
      const sources = new Set(rows.map((r) => (r.skill.source.kind === "installer" ? r.skill.source.source : "")));
      return `from ${count(sources.size, "GitHub repo")}`;
    }
    case "plugins": {
      const off = g.children.filter((c) => c.off).length;
      const on = g.children.length - off;
      const clis = CLIS.filter((c) => rows.some((r) => r.clis.includes(c)));
      return [cliText(clis), [on && `${on} on`, off && `${off} off`].filter(Boolean).join(", ")].join(" · ");
    }
    case "bundled":
      return CLIS.map((c) => [c, rows.filter((r) => r.skill.source.kind === "bundled" && r.skill.source.cli === c).length] as const)
        .filter(([, n]) => n > 0)
        .map(([c, n]) => `${CLI_TITLE[c]} ${n}`)
        .join(", ");
  }
  if (g.project !== null) {
    const added = rows.filter((r) => r.spaces.length > 0);
    if (added.length > 0) {
      const spaces = [...new Set(added.flatMap((r) => r.spaces))].sort((a, b) => a.localeCompare(b));
      return `${added.length} also in ${spaces.length === 1 ? "space" : "spaces"} ${and(spaces)}`;
    }
    if (rows.every((r) => r.skill.problems.some((p) => p.kind === "brokenLink"))) return "all broken links";
  }
  return g.note;
}

/// `sections` with only the skills whose name, any CLI's name for them,
/// description or group has every word of `query` in it, case aside, and
/// only the groups left with one.
export function filterTree(sections: Section[], query: string): Section[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return sections;
  const keep = (g: Group, above: string): Group | null => {
    const titles = `${above} ${g.title}`;
    const rows = g.rows.filter((r) => {
      const s = r.skill;
      const hay = [s.name, ...s.loads.map((l) => l.name), s.description ?? "", titles].join(" ").toLowerCase();
      return words.every((w) => hay.includes(w));
    });
    const children = g.children.map((c) => keep(c, titles)).filter((c): c is Group => c !== null);
    return rows.length + children.length > 0 ? { ...g, rows, children } : null;
  };
  return sections.map((sec) => ({ ...sec, groups: sec.groups.map((g) => keep(g, "")).filter((g): g is Group => g !== null) }));
}

/// A line of the list the cursor can be on: a group's head, or a skill.
export type Item = { kind: "group"; key: string; group: Group; parent: string | null } | { kind: "skill"; key: string; row: Row; parent: string };

/// The lines of `sections` on screen, the groups in `open` unfolded.
export function visibleItems(sections: Section[], open: ReadonlySet<string>): Item[] {
  const out: Item[] = [];
  const walk = (g: Group, parent: string | null) => {
    out.push({ kind: "group", key: g.key, group: g, parent });
    if (!open.has(g.key)) return;
    for (const c of g.children) walk(c, g.key);
    for (const row of g.rows) out.push({ kind: "skill", key: row.key, row, parent: g.key });
  };
  for (const sec of sections) for (const g of sec.groups) walk(g, null);
  return out;
}

/// Every group `sections` hold, plugins under Plugins included.
export function allGroups(sections: Section[]): Group[] {
  const walk = (g: Group): Group[] => [g, ...g.children.flatMap(walk)];
  return sections.flatMap((sec) => sec.groups.flatMap(walk));
}

/// Where the list shows skill `id` first: its section, and the group (and
/// the group above it) it is in.
export function placeOf(sections: Section[], id: string): { section: Section["key"]; path: Group[] } | null {
  for (const sec of sections) {
    for (const g of sec.groups) {
      if (g.rows.some((r) => r.skill.id === id)) return { section: sec.key, path: [g] };
      const child = g.children.find((c) => c.rows.some((r) => r.skill.id === id));
      if (child) return { section: sec.key, path: [g, child] };
    }
  }
  return null;
}

/// How many skills each CLI gives a session in `project`, which is filed
/// under `space`: every one it loads everywhere, the project's own, and
/// its space's added directories', less those switched off there, those
/// another skill of the same name wins over, and broken links.
export function sessionGets(skills: Skill[], project: string, space: string | null, cli: SkillCli): number {
  return skills.filter((s) => {
    const off = s.switches.some((w) => w.cli === cli && w.state === "off" && (w.project === null || w.project === project));
    return (
      !off &&
      !s.problems.some((p) => p.kind === "brokenLink") &&
      s.loads.some(
        (l) =>
          l.cli === cli &&
          !l.shadowedBy &&
          (isGlobal(l) ||
            (l.scope === "project" && l.project === project) ||
            (l.scope === "addedDir" && space !== null && l.space !== null && spaceKey(l.space) === spaceKey(space))),
      )
    );
  }).length;
}

/// Whether a setting switches `s` off for `cli` everywhere.
const offFor = (s: Skill, cli: SkillCli) => s.switches.some((w) => w.cli === cli && w.state === "off" && w.project === null);

export const STATE_TITLE: Record<SkillSwitch["state"], string> = {
  off: "Off",
  nameOnly: "Name only",
  userInvocableOnly: "Slash menu only",
};

/// What a CLI does with a skill, for its line under WHERE IT LOADS.
export interface CliLoad {
  cli: SkillCli;
  /// Where its sessions get the skill: `everywhere`, or the projects and
  /// spaces; null when none does.
  where: string | null;
  /// Why none does, when it is known.
  why: string | null;
  /// The skill it loads under that name instead.
  instead: Skill | null;
  /// Where it finds the skill, links and all.
  paths: string[];
  /// Its switch, in words; null for a CLI with none, or nothing to switch.
  state: string | null;
}

/// Where each CLI reads skills from, for a skill it does not.
const READS: Record<SkillCli, string> = {
  claude: "Claude reads ~/.claude/skills and a project's .claude/skills",
  codex: "Codex reads ~/.agents/skills, ~/.codex/skills and a project's .agents/skills",
  cursor: "Cursor reads the .agents, .cursor, .claude and .codex skills folders, at home and in a project",
};

/// What `cli` does with `s`, `all` being every skill on the machine.
export function whereItLoads(s: Skill, cli: SkillCli, all: Skill[], home: string): CliLoad {
  const loads = s.loads.filter((l) => l.cli === cli);
  const live = loads.filter((l) => !l.shadowedBy);
  const paths = [...new Set((live.length > 0 ? live : loads).map((l) => l.path))];
  const another = (other: Skill | undefined) => {
    const at = other?.loads.find((l) => l.cli === cli);
    return other && at ? `${CLI_TITLE[cli]} loads another \`${s.name}\`, from ${tilde(parent(at.path), home)}` : null;
  };
  if (live.length > 0) {
    const places = live.some(isGlobal)
      ? ["everywhere"]
      : [...new Set(live.map((l) => (l.project ? l.project : l.space ? `space ${l.space}` : "")).filter(Boolean))];
    return { cli, where: and(places), why: null, instead: null, paths, state: stateOf(s, cli) };
  }
  if (loads.length > 0) {
    const instead = all.find((x) => x.id === loads[0]!.shadowedBy);
    return { cli, where: null, why: another(instead), instead: instead ?? null, paths, state: null };
  }
  const instead = all.find((x) => x.id !== s.id && x.loads.some((l) => l.cli === cli && l.name === s.name));
  const src = s.source;
  const why =
    another(instead) ??
    (src.kind === "plugin"
      ? "a plugin is Claude's"
      : src.kind === "claudeAi"
        ? "only Claude syncs claude.ai skills"
        : src.kind === "bundled"
          ? `it comes with ${CLI_TITLE[src.cli]}`
          : s.kind === "command"
            ? "a command is Claude's"
            : READS[cli]);
  return { cli, where: null, why, instead: instead ?? null, paths: [], state: null };
}

/// `cli`'s switch for `s` in words: `On`, `Off`, `Name only, off in shop`.
function stateOf(s: Skill, cli: SkillCli): string | null {
  if (cli === "cursor") return null;
  const mine = s.switches.filter((w) => w.cli === cli);
  const everywhere = mine.find((w) => w.project === null);
  const projects = mine.filter((w) => w.project !== null && w.state !== everywhere?.state);
  const head = everywhere ? STATE_TITLE[everywhere.state] : "On";
  return [head, ...projects.map((w) => `${STATE_TITLE[w.state].toLowerCase()} in ${w.project}`)].join(", ");
}

/// What a switch says, after its state and CLI: the setting, and the file
/// it is in.
export function switchText(w: SkillSwitch, home: string): string {
  if (w.file === null) return `no settings file turns the plugin on in \`${w.setting}\``;
  return `\`${w.setting}\` in ${tilde(w.file, home)}`;
}

/// A frontmatter value as text: a scalar as it is, a list of scalars
/// comma-separated, anything deeper as YAML-like lines.
export function valueText(v: FrontmatterValue, indent = ""): string {
  if (v === null) return "null";
  if (typeof v !== "object") return String(v);
  if (Array.isArray(v)) {
    if (v.every((x) => x === null || typeof x !== "object")) return v.map((x) => valueText(x)).join(", ");
    return v.map((x) => `${indent}- ${valueText(x, `${indent}  `).trimStart()}`).join("\n");
  }
  return Object.entries(v)
    .map(([k, x]) => {
      const nested = x !== null && typeof x === "object" && !(Array.isArray(x) && x.every((y) => typeof y !== "object"));
      return nested ? `${indent}${k}:\n${valueText(x, `${indent}  `)}` : `${indent}${k}: ${valueText(x)}`;
    })
    .join("\n");
}

/// `n` skills or files, in words.
export function count(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}

/// The overview's one line: how many skills for which CLIs, how many load
/// everywhere and how many in a project, and how many things need fixing.
export function skillsSummary(sections: Section[], skills: Skill[], clis: SkillCli[], fixes: number): string {
  const [global, local] = sections.map((sec) => skillCount(sec.groups.flatMap(rowsOf)));
  const projects = sections[1]!.groups.filter((g) => g.project !== null).length;
  const names = clis.map((c) => CLI_TITLE[c]);
  const head = `${count(skills.length, "skill")} for ${names.length > 0 ? and(names) : "no CLI"}`;
  const where =
    local === 0
      ? `${head}, all of them in every project.`
      : `${head}: ${global} load in every project, ${local} in ${projects === 1 ? "one project" : `one of ${projects} projects`}.`;
  const fixing = fixes === 0 ? "Nothing needs fixing." : `${fixes === 1 ? "One thing needs" : `${fixes} things need`} fixing.`;
  return `${where} ${fixing}`;
}

/// Each skill Claude used lately, by id.
export type Uses = ReadonlyMap<string, SkillUse>;

export const usesOf = (usage: SkillUsage | null): Uses => new Map(usage?.used.map((u) => [u.id, u]));

/// What the list's quiet column needs: the uses, and the time it is.
export interface Used {
  uses: Uses;
  now: number;
}

/// How many days ago `at` was, by the calendar: `today`, `1d`, `14d`.
export function usedLabel(at: number, now: number): string {
  const day = (t: number) => {
    const d = new Date(t);
    return Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()) / 86_400_000;
  };
  const days = Math.max(0, Math.round(day(now) - day(at)));
  return days === 0 ? "today" : `${days}d`;
}

/// When a row's skill was used last, for a row Claude loads it on.
export function rowUsed(r: Row, uses: Uses): number | null {
  return r.clis.includes("claude") ? (uses.get(r.skill.id)?.last ?? null) : null;
}

/// When the skill of `g` used most recently was used.
export function groupUsed(g: Group, uses: Uses): number | null {
  return rowsOf(g).reduce<number | null>((last, r) => {
    const at = rowUsed(r, uses);
    return at !== null && (last === null || at > last) ? at : last;
  }, null);
}

/// What a skill's page says of its use: `2 times in 30 days, last on 2 Nov
/// in shop`, or why there is nothing to say.
export function usedText(s: Skill, usage: SkillUsage, uses: Uses): { text: string; quiet: boolean } {
  const u = uses.get(s.id);
  const days = `in ${usage.days} days`;
  if (u) {
    const day = new Date(u.last).toLocaleDateString("en-GB", { day: "numeric", month: "short" });
    const where = u.project ? ` in ${u.project}` : "";
    const text = u.count === 1 ? `once ${days}, on ${day}${where}` : `${u.count} times ${days}, last on ${day}${where}`;
    return { text, quiet: false };
  }
  if (s.loads.some((l) => l.cli === "claude")) return { text: `not ${days}`, quiet: true };
  return { text: `not read for ${and(clisIn(s.loads).map((c) => CLI_TITLE[c]))}`, quiet: true };
}

/// Why a skill's use is not counted, if it is not: Claude does not load
/// it, or not as it stands.
function uncounted(s: Skill): "builtIn" | "others" | "broken" | "off" | "hidden" | null {
  const claude = s.loads.filter((l) => l.cli === "claude");
  if (s.source.kind === "bundled") return "builtIn";
  if (claude.length === 0) return "others";
  if (s.problems.some((p) => p.kind === "brokenLink")) return "broken";
  if (offFor(s, "claude")) return "off";
  if (claude.every((l) => l.shadowedBy)) return "hidden";
  return null;
}

/// Where Claude loads a skill, in a word: `global`, `plugin`, `claude.ai`,
/// or its projects.
function loadsWhere(s: Skill): string {
  const all = s.loads.filter((l) => l.cli === "claude");
  const live = all.some((l) => !l.shadowedBy) ? all.filter((l) => !l.shadowedBy) : all;
  if (live.some((l) => l.scope === "plugin")) return "plugin";
  if (live.some((l) => l.scope === "claudeAi")) return "claude.ai";
  if (live.some(isGlobal)) return "global";
  return and([...new Set(live.map((l) => l.project ?? (l.space ? `space ${l.space}` : "")).filter(Boolean))]);
}

export interface UseSummary {
  /// Every skill used, most used first: what Claude calls it, and where it
  /// loads.
  most: { skill: Skill; name: string; where: string; count: number }[];
  /// For each place of what Claude loads, how many of its skills were not
  /// used, a place with none left out.
  notUsed: { title: string; unused: number; of: number }[];
  /// How many skills Claude loads were not used.
  unused: number;
  /// What is not counted, as a sentence.
  notCounted: string | null;
}

/// The overview's use, last 30 days: the skills most used, and in each
/// place (yours, each plugin, claude.ai, the projects) how many of the
/// skills Claude loads were not.
export function useSummary(sections: Section[], skills: Skill[], clis: SkillCli[], usage: SkillUsage, uses: Uses): UseSummary {
  const byId = new Map(skills.map((s) => [s.id, s]));
  const most = usage.used.flatMap((u) => {
    const s = byId.get(u.id);
    const name = s?.loads.find((l) => l.cli === "claude")?.name ?? s?.name;
    return s && name ? [{ skill: s, name, where: loadsWhere(s), count: u.count }] : [];
  });
  const counted = (rows: Row[]) => new Set(rows.filter((r) => r.clis.includes("claude") && !uncounted(r.skill)).map((r) => r.skill.id));
  const [global, local] = sections;
  const top = (key: string) => global?.groups.find((g) => g.key === key);
  const places: [string, Row[]][] = [
    ["Yours, global", global?.groups.filter((g) => !["plugins", "claudeAi", "bundled"].includes(g.key)).flatMap(rowsOf) ?? []],
    ...(top("plugins")?.children ?? []).map((c): [string, Row[]] => [`${c.title} plugin`, rowsOf(c)]),
    ["claude.ai", top("claudeAi") ? rowsOf(top("claudeAi")!) : []],
    ["Projects", local?.groups.flatMap(rowsOf) ?? []],
  ];
  const notUsed = places.flatMap(([title, rows]) => {
    const ids = [...counted(rows)];
    const unused = ids.filter((id) => !uses.has(id)).length;
    return unused > 0 ? [{ title, unused, of: ids.length }] : [];
  });
  const why = new Map<string, number>();
  for (const s of skills) {
    const w = uncounted(s);
    if (w) why.set(w, (why.get(w) ?? 0) + 1);
  }
  const others = clis.filter((c) => c !== "claude").map((c) => CLI_TITLE[c]);
  const parts = [
    ["off", (n: number) => `${n} switched off`],
    ["builtIn", (n: number) => `${n} built in`],
    ["broken", (n: number) => `${count(n, "broken link")}`],
    ["hidden", (n: number) => `${n} hidden by another of the same name`],
    ["others", (n: number) => `${n} of yours that only ${and(others)} ${others.length === 1 ? "loads" : "load"}`],
  ] as const;
  const said = parts.flatMap(([w, text]) => (why.get(w) ? [text(why.get(w)!)] : []));
  return {
    most,
    notUsed,
    unused: skills.filter((s) => s.loads.some((l) => l.cli === "claude") && !uncounted(s) && !uses.has(s.id)).length,
    notCounted: said.length > 0 ? `Not counted: ${and(said)}.` : null,
  };
}

/// What a key does on the Skills page. In the filter, letters are typed
/// and the up and down arrows, Enter and Esc still drive the list; Esc
/// clears the filter before it leaves it.
export type SkillAction =
  | { kind: "move"; delta: 1 | -1 }
  | { kind: "fold"; open: boolean }
  | { kind: "open" }
  | { kind: "back" }
  | { kind: "filter" }
  | { kind: "clear" }
  | { kind: "leave" };

export function skillKey(
  e: { key: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean },
  inFilter: boolean,
  query: string,
): SkillAction | null {
  if (inFilter) {
    switch (e.key) {
      case "ArrowDown":
        return { kind: "move", delta: 1 };
      case "ArrowUp":
        return { kind: "move", delta: -1 };
      case "Enter":
        return { kind: "open" };
      case "Escape":
        return { kind: query ? "clear" : "leave" };
    }
    return null;
  }
  if (e.metaKey || e.ctrlKey || e.altKey) return null;
  if (e.key === "/") return { kind: "filter" };
  if (e.key === "ArrowLeft" || e.key === "h") return { kind: "fold", open: false };
  if (e.key === "ArrowRight" || e.key === "l") return { kind: "fold", open: true };
  const action = keyAction(e);
  return action?.kind === "move" || action?.kind === "open" || action?.kind === "back" ? action : null;
}

/// The skill a filter means most: one named as typed, else one whose name
/// starts with it, else one whose name holds it, else the first it left.
function bestMatch(items: Skill[], query: string): Skill | undefined {
  const q = query.trim().toLowerCase();
  if (!q) return undefined;
  const rank = (s: Skill) => {
    const name = s.name.toLowerCase();
    return name === q ? 0 : name.startsWith(q) ? 1 : name.includes(q) ? 2 : 3;
  };
  return items.reduce<Skill | undefined>((best, s) => (best && rank(best) <= rank(s) ? best : s), undefined);
}

/// The line a filter puts the cursor on: the row of the skill it means most.
export function bestItem(items: Item[], query: string): Item | undefined {
  const rows = items.flatMap((i) => (i.kind === "skill" ? [i] : []));
  const best = bestMatch(
    rows.map((i) => i.row.skill),
    query,
  );
  return rows.find((i) => i.row.skill === best);
}
