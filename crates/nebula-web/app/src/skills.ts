import type { FrontmatterValue, Skill, SkillCli, SkillLoad, SkillScope, SkillSwitch } from "./api/contract";
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
export const CLI_SHORT: Record<SkillCli, string> = { claude: "cl", codex: "cx", cursor: "cu" };

/// Where a skill comes from, as the list groups it: what owns the folder.
export interface Group {
  key: string;
  title: string;
  /// What the title stands for: the repository's path and branch, a
  /// plugin's marketplace.
  note: string;
  skills: Skill[];
}

/// The order groups come in: the user's own first, what was installed
/// next, and what a CLI brought last.
const RANK: Record<Skill["source"]["kind"], number> = {
  repo: 0,
  folder: 1,
  installer: 2,
  plugin: 3,
  claudeAi: 4,
  bundled: 5,
};

export function groupOf(s: Skill, home: string): Omit<Group, "skills"> {
  const src = s.source;
  switch (src.kind) {
    case "repo": {
      const root = s.repo?.root ?? parent(s.path);
      const branch = s.repo?.branch;
      return { key: `repo:${root}`, title: base(root), note: branch ? `${tilde(root, home)} · ${branch}` : tilde(root, home) };
    }
    case "folder": {
      const dir = parent(s.path);
      return { key: `folder:${dir}`, title: tilde(dir, home), note: "In no git repository" };
    }
    case "installer":
      return { key: "installer", title: "npx skills", note: "Installed from GitHub" };
    case "plugin":
      return {
        key: `plugin:${src.plugin}@${src.marketplace}`,
        title: src.plugin,
        note: `Plugin from ${src.marketplace}${src.version ? ` · ${src.version}` : ""}`,
      };
    case "claudeAi":
      return { key: "claudeAi", title: "claude.ai", note: "Synced from the account" };
    case "bundled":
      return { key: `bundled:${src.cli}`, title: CLI_TITLE[src.cli], note: "Bundled with the CLI" };
  }
}

/// `skills` in their groups, in the order groups come in, each by name.
export function groups(skills: Skill[], home: string): Group[] {
  const byKey = new Map<string, Group & { rank: number }>();
  for (const s of skills) {
    const g = groupOf(s, home);
    let group = byKey.get(g.key);
    if (!group) byKey.set(g.key, (group = { ...g, skills: [], rank: RANK[s.source.kind] }));
    group.skills.push(s);
  }
  const name = (s: Skill) => s.name.toLowerCase();
  return [...byKey.values()]
    .sort((a, b) => a.rank - b.rank || a.title.localeCompare(b.title))
    .map(({ rank: _rank, ...g }) => ({ ...g, skills: g.skills.sort((a, b) => name(a).localeCompare(name(b))) }));
}

/// The skills whose name, any CLI's name for them, or group has every word
/// of `query` in it, case aside.
export function filterSkills(skills: Skill[], query: string, home: string): Skill[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return skills;
  return skills.filter((s) => {
    const hay = [s.name, ...s.loads.map((l) => l.name), groupOf(s, home).title].join(" ").toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

/// The CLIs that load `s`, in a fixed order.
export function clisOf(s: Skill): SkillCli[] {
  const has = new Set(s.loads.map((l) => l.cli));
  return (["claude", "codex", "cursor"] as const).filter((c) => has.has(c));
}

/// Whether some setting switches `s` off for `cli`, everywhere or in a project.
export function offFor(s: Skill, cli: SkillCli): SkillSwitch | undefined {
  return s.switches.find((w) => w.cli === cli && w.state === "off" && w.project === null);
}

const SCOPE: Record<SkillScope, string> = {
  managed: "managed",
  personal: "personal",
  project: "project",
  nested: "nested",
  addedDir: "added dir",
  plugin: "plugin",
  claudeAi: "claude.ai",
  admin: "admin",
  system: "bundled",
};

/// Where a load is, in a few words: its scope, and the project or space it
/// is for.
export function whereOf(l: SkillLoad): string {
  if (l.scope === "nested" && l.project) return `nested in ${l.project}`;
  if (l.project) return `${SCOPE[l.scope]} ${l.project}`;
  if (l.space) return `space ${l.space}`;
  return SCOPE[l.scope];
}

export const STATE_TITLE: Record<SkillSwitch["state"], string> = {
  off: "Off",
  nameOnly: "Name only",
  userInvocableOnly: "User only",
};

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

/// The overview's one line: how many skills, from how many places, for
/// which CLIs, and how many have a problem.
export function skillsSummary(skills: Skill[], clis: SkillCli[], places: number): string {
  const troubled = skills.filter((s) => s.problems.length > 0).length;
  const names = clis.map((c) => CLI_TITLE[c]);
  const who = names.length > 1 ? `${names.slice(0, -1).join(", ")} and ${names.at(-1)}` : (names[0] ?? "no CLI");
  const from = `${count(skills.length, "skill")} from ${count(places, "place")}, for ${who}.`;
  return troubled === 0 ? `${from} None has a problem.` : `${from} ${troubled === 1 ? "One has" : `${troubled} have`} a problem.`;
}

/// What a key does on the Skills page. In the filter, letters are typed
/// and the arrows, Enter and Esc still drive the list; Esc clears the
/// filter before it leaves it.
export type SkillAction =
  | { kind: "move"; delta: 1 | -1 }
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
  if (e.key === "/" && !e.metaKey && !e.ctrlKey && !e.altKey) return { kind: "filter" };
  const action = keyAction(e);
  return action?.kind === "move" || action?.kind === "open" || action?.kind === "back" ? action : null;
}

/// The skill a filter means most: one named as typed, else one whose name
/// starts with it, else one whose name holds it, else the first it left.
export function bestMatch(items: Skill[], query: string): Skill | undefined {
  const q = query.trim().toLowerCase();
  if (!q) return undefined;
  const rank = (s: Skill) => {
    const name = s.name.toLowerCase();
    return name === q ? 0 : name.startsWith(q) ? 1 : name.includes(q) ? 2 : 3;
  };
  return items.reduce<Skill | undefined>((best, s) => (best && rank(best) <= rank(s) ? best : s), undefined);
}

/// The skill Enter opens: the one under the cursor, else the one a filter
/// means most.
export function skillToOpen(items: Skill[], cursor: string | null, query: string): Skill | undefined {
  return items.find((s) => s.id === cursor) ?? bestMatch(items, query);
}
