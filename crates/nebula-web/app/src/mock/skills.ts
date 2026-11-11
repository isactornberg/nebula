// Made-up skills for `pnpm dev`, one of each kind the page shows. Nothing
// here is a real skill or a real machine's folder.
import type {
  ApiError,
  GitRepo,
  Skill,
  SkillControl,
  SkillDetail,
  SkillField,
  SkillFix,
  SkillLoad,
  SkillProblem,
  SkillSource,
  SkillSwitch,
  Skills,
  SkillTo,
  SkillUsage,
  SkillWrite,
} from "../api/contract.ts";

const HOME = "/Users/you";
const TOOLBOX: GitRepo = { root: `${HOME}/code/toolbox`, branch: "main" };
const SHOP: GitRepo = { root: `${HOME}/code/shopfront`, branch: "checkout-retries" };
const LEDGER: GitRepo = { root: `${HOME}/code/ledger`, branch: "main" };
const DOCS: GitRepo = { root: `${HOME}/code/docs-site`, branch: "main" };
const ROVER: GitRepo = { root: `${HOME}/code/rover-firmware`, branch: "main" };
const HANDBOOK: GitRepo = { root: `${HOME}/code/handbook`, branch: "main" };

interface MockSkill {
  skill: Skill;
  fields: SkillField[];
  body: string | null;
  files: string[];
}

function load(
  cli: SkillLoad["cli"],
  scope: SkillLoad["scope"],
  path: string,
  name: string,
  project: string | null = null,
  more: Partial<SkillLoad> = {},
): SkillLoad {
  return { cli, name, scope, path, project, space: null, shadowedBy: null, ...more };
}

let next = 0;

function mock(s: {
  name: string;
  description: string | null;
  path: string;
  source?: SkillSource;
  repo?: GitRepo;
  kind?: Skill["kind"];
  loads: SkillLoad[];
  switches?: SkillSwitch[];
  problems?: SkillProblem[];
  fields?: SkillField[];
  body?: string | null;
  files?: string[];
}): MockSkill {
  next += 1;
  const fields = s.fields ?? [
    { key: "name", value: s.name },
    ...(s.description === null ? [] : [{ key: "description", value: s.description }]),
  ];
  return {
    skill: {
      id: next.toString(16).padStart(16, "0"),
      name: s.name,
      description: s.description,
      kind: s.kind ?? "skill",
      path: s.path,
      source: s.source ?? (s.repo ? { kind: "repo" } : { kind: "folder" }),
      repo: s.repo ?? null,
      loads: s.loads,
      switches: s.switches ?? [],
      controls: [],
      problems: s.problems ?? [],
    },
    fields: s.kind === "command" ? [] : fields,
    body:
      s.body === undefined
        ? `# ${s.name}\n\n${s.description ?? ""}\n\n## Steps\n\n1. Read what the user asked for.\n2. Do it, and say what changed.\n\n\`\`\`sh\n./scripts/run.sh --check\n\`\`\`\n`
        : s.body,
    files: s.files ?? ["SKILL.md"],
  };
}

const personal = (name: string) => `${HOME}/.claude/skills/${name}`;
const agents = (name: string) => `${HOME}/.agents/skills/${name}`;

let deploy: MockSkill;

export const skills: MockSkill[] = [
  mock({
    name: "capture",
    description: "File one thing to act on in the task inbox. Use when the user says capture this.",
    path: `${TOOLBOX.root}/skills/capture`,
    repo: TOOLBOX,
    loads: [
      load("claude", "personal", personal("capture"), "capture"),
      load("codex", "personal", agents("capture"), "capture"),
      load("cursor", "personal", agents("capture"), "capture"),
      load("cursor", "personal", personal("capture"), "capture"),
    ],
    switches: [
      { cli: "codex", state: "off", setting: "skills.config", file: `${HOME}/.codex/config.toml`, project: null },
    ],
    fields: [
      { key: "name", value: "capture" },
      { key: "description", value: "File one thing to act on in the task inbox. Use when the user says capture this." },
      { key: "allowed-tools", value: ["Bash(todo add *)", "Read"] },
      { key: "disable-model-invocation", value: true },
      { key: "metadata", value: { owner: "you", version: 3 } },
    ],
    files: ["SKILL.md", "scripts/add.sh", "reference/inbox.md"],
  }),
  mock({
    name: "release-notes",
    description: "Write the release notes for a tag from the merged pull requests since the last one.",
    path: `${TOOLBOX.root}/skills/release-notes`,
    repo: TOOLBOX,
    loads: [
      load("claude", "personal", personal("release-notes"), "release-notes"),
      load("cursor", "personal", personal("release-notes"), "release-notes"),
    ],
    switches: [
      {
        cli: "claude",
        state: "nameOnly",
        setting: "skillOverrides",
        file: `${HOME}/.claude/settings.json`,
        project: null,
      },
    ],
  }),
  (deploy = mock({
    name: "deploy",
    description: "Ship the current branch to staging, then to production once the checks pass.",
    path: `${TOOLBOX.root}/skills/deploy`,
    repo: TOOLBOX,
    loads: [
      load("claude", "personal", personal("deploy"), "deploy"),
      load("cursor", "personal", personal("deploy"), "deploy"),
    ],
    problems: [
      {
        kind: "duplicateName",
        text: "Claude finds another skill named `deploy`, ~/code/shopfront/.claude/skills/deploy (project shopfront), and uses this one.",
      },
      {
        kind: "duplicateName",
        text: "Cursor finds another skill named `deploy`, ~/code/shopfront/.claude/skills/deploy (project shopfront); it documents no order between them.",
      },
    ],
  })),
  mock({
    name: "changelog",
    description: "Write the changelog entry for the change on this branch.",
    path: `${TOOLBOX.root}/skills/changelog`,
    repo: TOOLBOX,
    loads: [
      load("codex", "personal", agents("changelog"), "changelog"),
      load("cursor", "personal", agents("changelog"), "changelog"),
    ],
  }),
  mock({
    name: "deploy",
    description: "Deploy the shop with its own scripts.",
    path: `${SHOP.root}/.claude/skills/deploy`,
    repo: SHOP,
    loads: [
      load("claude", "project", `${SHOP.root}/.claude/skills/deploy`, "deploy", "shopfront", { shadowedBy: deploy.skill.id }),
      load("cursor", "project", `${SHOP.root}/.claude/skills/deploy`, "deploy", "shopfront"),
    ],
    problems: [
      {
        kind: "duplicateName",
        text: "Claude finds another skill named `deploy`, ~/.claude/skills/deploy (personal), and uses ~/.claude/skills/deploy (personal) instead.",
      },
      {
        kind: "duplicateName",
        text: "Cursor finds another skill named `deploy`, ~/.claude/skills/deploy (personal); it documents no order between them.",
      },
    ],
  }),
  mock({
    name: "translate",
    description: "Translate the shop's strings into every language it ships in.",
    path: `${SHOP.root}/.claude/skills/translate`,
    repo: SHOP,
    loads: [
      load("claude", "project", `${SHOP.root}/.claude/skills/translate`, "translate", "shopfront"),
      load("cursor", "project", `${SHOP.root}/.claude/skills/translate`, "translate", "shopfront"),
    ],
    files: ["SKILL.md", "glossary.csv"],
  }),
  mock({
    name: "review",
    kind: "command",
    description: "Review the diff against main.",
    path: `${SHOP.root}/.claude/commands/review.md`,
    repo: SHOP,
    loads: [load("claude", "project", `${SHOP.root}/.claude/commands/review.md`, "review", "shopfront")],
    body: "Review the diff against main, and say what would break.\n",
    files: ["review.md"],
  }),
  ...["vendor-auth", "vendor-billing", "vendor-cache", "vendor-search"].map((name) =>
    mock({
      name,
      description: null,
      path: `${LEDGER.root}/.claude/skills/${name}`,
      repo: LEDGER,
      loads: [
        load("claude", "project", `${LEDGER.root}/.claude/skills/${name}`, name, "ledger"),
        load("cursor", "project", `${LEDGER.root}/.claude/skills/${name}`, name, "ledger"),
      ],
      problems: [
        { kind: "brokenLink", text: `The link points at ~/code/ledger/.agents/skills/${name}, which is not there.` },
      ],
      body: null,
      files: [],
    }),
  ),
  mock({
    name: "style-guide",
    description: "Hold the docs to the house style: plain words, one idea a sentence.",
    path: `${DOCS.root}/.claude/skills/style-guide`,
    repo: DOCS,
    loads: [
      load("claude", "project", `${DOCS.root}/.claude/skills/style-guide`, "style-guide", "docs-site"),
      load("cursor", "project", `${DOCS.root}/.claude/skills/style-guide`, "style-guide", "docs-site"),
      load("claude", "addedDir", `${DOCS.root}/.claude/skills/style-guide`, "style-guide", null, { space: "Acme" }),
    ],
  }),
  mock({
    name: "onboarding",
    description: "Walk a new teammate through the handbook, one chapter a day.",
    path: `${HANDBOOK.root}/.claude/skills/onboarding`,
    repo: HANDBOOK,
    loads: [load("claude", "addedDir", `${HANDBOOK.root}/.claude/skills/onboarding`, "onboarding", null, { space: "Acme" })],
    problems: [{ kind: "oneCli", text: "Only Claude loads it." }],
  }),
  mock({
    name: "flash-board",
    description: "Build the firmware and flash it onto the board on the bench.",
    path: `${ROVER.root}/.agents/skills/flash-board`,
    repo: ROVER,
    loads: [
      load("codex", "project", `${ROVER.root}/.agents/skills/flash-board`, "flash-board", "rover-firmware"),
      load("cursor", "project", `${ROVER.root}/.agents/skills/flash-board`, "flash-board", "rover-firmware"),
    ],
  }),
  mock({
    name: "standup",
    description: "Write the stand-up note from yesterday's commits and today's calendar.",
    path: `${TOOLBOX.root}/skills/standup`,
    repo: TOOLBOX,
    loads: [
      load("claude", "personal", personal("standup"), "standup"),
      load("cursor", "personal", personal("standup"), "standup"),
    ],
  }),
  mock({
    name: "weekly-review",
    description: "Sum up the week from the merged pull requests and the closed issues.",
    path: `${TOOLBOX.root}/skills/weekly-review`,
    repo: TOOLBOX,
    loads: [
      load("claude", "personal", personal("weekly-review"), "weekly-review"),
      load("cursor", "personal", personal("weekly-review"), "weekly-review"),
    ],
  }),
  mock({
    name: "sketchpad",
    description: "Draw a quick diagram of the code the user points at, as an SVG.",
    path: personal("sketchpad"),
    loads: [
      load("claude", "personal", personal("sketchpad"), "sketchpad"),
      load("cursor", "personal", personal("sketchpad"), "sketchpad"),
    ],
    files: ["SKILL.md", "templates/box.svg", "templates/flow.svg"],
  }),
  mock({
    name: "Tidy_Up",
    description: "Tidy the imports and the formatting of the files the turn touched.",
    path: agents("tidy"),
    loads: [load("codex", "personal", agents("tidy"), "Tidy_Up"), load("cursor", "personal", agents("tidy"), "Tidy_Up")],
    problems: [
      { kind: "nameMismatch", text: "The frontmatter names it `Tidy_Up`, but its folder is `tidy`." },
      {
        kind: "badName",
        text: "The name `Tidy_Up` breaks the Agent Skills rule: at most 64 characters, lowercase letters, digits and single hyphens.",
      },
    ],
  }),
  mock({
    name: "skill-creator",
    description: "Create a new skill, or improve one, and measure how often it triggers.",
    path: agents("skill-creator"),
    source: { kind: "installer", source: "example/skills", url: "https://github.com/example/skills.git", path: "skills/skill-creator/SKILL.md" },
    loads: [
      load("claude", "personal", personal("skill-creator"), "skill-creator"),
      load("codex", "personal", agents("skill-creator"), "skill-creator"),
      load("cursor", "personal", agents("skill-creator"), "skill-creator"),
    ],
    problems: [
      {
        kind: "duplicateName",
        text: "Codex finds another skill named `skill-creator`, ~/.codex/skills/.system/skill-creator (bundled), and lists both.",
      },
    ],
    files: ["SKILL.md", "scripts/eval.py", "agents/openai.yaml"],
  }),
  mock({
    name: "lint",
    description: "Run the project's linters and fix what they flag.",
    path: `${HOME}/.claude/plugins/cache/tools-market/lint-kit/2.1.0/skills/lint`,
    source: { kind: "plugin", plugin: "lint-kit", version: "2.1.0", marketplace: "tools-market", from: "github.com/example/tools" },
    loads: [load("claude", "plugin", `${HOME}/.claude/plugins/cache/tools-market/lint-kit/2.1.0/skills/lint`, "lint-kit:lint")],
  }),
  mock({
    name: "format",
    description: "Format the files the turn touched.",
    path: `${HOME}/.claude/plugins/cache/tools-market/lint-kit/2.1.0/skills/format`,
    source: { kind: "plugin", plugin: "lint-kit", version: "2.1.0", marketplace: "tools-market", from: "github.com/example/tools" },
    loads: [load("claude", "plugin", `${HOME}/.claude/plugins/cache/tools-market/lint-kit/2.1.0/skills/format`, "lint-kit:format")],
  }),
  mock({
    name: "notebook",
    description: "Keep a running log of what was decided, in the project's notes folder.",
    path: `${HOME}/code/plugins/notebook/skills/notebook`,
    repo: { root: `${HOME}/code/plugins`, branch: "main" },
    source: { kind: "plugin", plugin: "notebook", version: null, marketplace: "local", from: `${HOME}/code/plugins` },
    loads: [load("claude", "plugin", `${HOME}/code/plugins/notebook/skills/notebook`, "notebook:notebook")],
    switches: [{ cli: "claude", state: "off", setting: "enabledPlugins", file: null, project: null }],
  }),
  mock({
    name: "docs",
    description: "Make a document people share and comment on.",
    path: `${HOME}/.claude/skills/synced/bucket/docs`,
    source: { kind: "claudeAi" },
    loads: [load("claude", "claudeAi", `${HOME}/.claude/skills/synced/bucket/docs`, "anthropic-skills:docs")],
  }),
  mock({
    name: "imagegen",
    description: "Generate an image from a prompt.",
    path: `${HOME}/.codex/skills/.system/imagegen`,
    source: { kind: "bundled", cli: "codex" },
    loads: [load("codex", "system", `${HOME}/.codex/skills/.system/imagegen`, "imagegen")],
  }),
  mock({
    name: "skill-creator",
    description: "Create a skill for Codex.",
    path: `${HOME}/.codex/skills/.system/skill-creator`,
    source: { kind: "bundled", cli: "codex" },
    loads: [load("codex", "system", `${HOME}/.codex/skills/.system/skill-creator`, "skill-creator")],
    problems: [
      {
        kind: "duplicateName",
        text: "Codex finds another skill named `skill-creator`, ~/.agents/skills/skill-creator (personal), and lists both.",
      },
    ],
  }),
  mock({
    name: "create-rule",
    description: "Write a Cursor rule for the project.",
    path: `${HOME}/.cursor/skills-cursor/create-rule`,
    source: { kind: "bundled", cli: "cursor" },
    loads: [load("cursor", "system", `${HOME}/.cursor/skills-cursor/create-rule`, "create-rule")],
  }),
];

const id = (path: string) => skills.find((m) => m.skill.path === path)!.skill.id;

/// What `nebula serve` lists to fix for the skills above.
const fixes: SkillFix[] = [
  {
    kind: "brokenLink",
    title: "4 broken links",
    place: "in ledger",
    text: "4 links in .claude/skills point into .agents/skills, which is not there.",
    fix: "Remove the links, or put back .agents/skills.",
    skills: ["vendor-auth", "vendor-billing", "vendor-cache", "vendor-search"].map((n) => id(`${LEDGER.root}/.claude/skills/${n}`)),
  },
  {
    kind: "duplicateName",
    title: "deploy twice",
    place: "in Claude and Cursor, in shopfront",
    text: "Claude uses ~/.claude/skills/deploy over .claude/skills/deploy. Cursor finds both and documents no order between them.",
    fix: "Rename one of them, or remove the one you do not use.",
    skills: [id(`${TOOLBOX.root}/skills/deploy`), id(`${SHOP.root}/.claude/skills/deploy`)],
  },
  {
    kind: "duplicateName",
    title: "skill-creator twice",
    place: "in Codex",
    text: "Codex lists ~/.agents/skills/skill-creator and the built-in one.",
    fix: "Rename yours, or switch the built-in one off in Codex's settings.",
    skills: [id(agents("skill-creator")), id(`${HOME}/.codex/skills/.system/skill-creator`)],
  },
  {
    kind: "nameMismatch",
    title: "Tidy_Up",
    place: "in ~/.agents/skills",
    text: "The frontmatter names it `Tidy_Up`, but its folder is `tidy`. The name `Tidy_Up` breaks the Agent Skills rule: at most 64 characters, lowercase letters, digits and single hyphens.",
    fix: "Give the frontmatter and the folder one name. Rename it in lowercase letters, digits and single hyphens.",
    skills: [id(agents("tidy"))],
  },
  {
    kind: "oneCli",
    title: "onboarding",
    place: "for Claude only",
    text: "Codex and Cursor do not find it where they look.",
    fix: "Link it into a folder Codex and Cursor read, if they should load it too.",
    skills: [id(`${HANDBOOK.root}/.claude/skills/onboarding`)],
  },
];

const tilde = (path: string) => (path.startsWith(HOME) ? `~${path.slice(HOME.length)}` : path);

const GLOBAL = new Set<SkillLoad["scope"]>(["managed", "personal", "plugin", "claudeAi", "admin", "system"]);

/// The switches the page can set for `s`, as `nebula serve` works them out:
/// a plugin's for its skills, Claude's own in the user's settings or a
/// project's local ones, and Codex's.
function controlsOf(s: Skill): SkillControl[] {
  if (s.problems.some((p) => p.kind === "brokenLink")) return [];
  const out: SkillControl[] = [];
  const off = (cli: SkillLoad["cli"], setting: string, project: string | null = null) =>
    s.switches.find((w) => w.cli === cli && w.setting === setting && w.project === project);
  const live = s.loads.filter((l) => l.cli === "claude" && !l.shadowedBy);
  const at = (setting: SkillControl["setting"], file: string, state: SkillControl["state"], more: Partial<SkillControl> = {}) =>
    out.push({ cli: "claude", setting, file, state, plugin: null, project: null, ...more });
  if (s.source.kind === "plugin") {
    at("enabledPlugins", "~/.claude/settings.json", off("claude", "enabledPlugins") ? "off" : "on", {
      plugin: `${s.source.plugin}@${s.source.marketplace}`,
    });
  } else if (live.some((l) => GLOBAL.has(l.scope))) {
    at("skillOverrides", "~/.claude/settings.json", off("claude", "skillOverrides")?.state ?? "on");
  } else if (live.length > 0 && live.every((l) => l.scope === "project" && l.project === live[0]!.project)) {
    const project = live[0]!.project!;
    const root = tilde(live[0]!.path.slice(0, live[0]!.path.indexOf("/.claude/")));
    at("skillOverrides", `${root}/.claude/settings.local.json`, off("claude", "skillOverrides", project)?.state ?? "on", { project });
  }
  if (s.loads.some((l) => l.cli === "codex")) {
    out.push({
      cli: "codex",
      setting: "skills.config",
      file: "~/.codex/config.toml",
      state: off("codex", "skills.config") ? "off" : "on",
      plugin: null,
      project: null,
    });
  }
  return out;
}

for (const m of skills) m.skill.controls = controlsOf(m.skill);

/// The made-up settings files: the user's is a link into a dotfiles
/// repository, and one project's local file does not parse, so the page's
/// refusal can be seen.
function written(file: string, changed: boolean, before: SkillTo[] = []): SkillWrite {
  const dotfiles = file === "~/.claude/settings.json";
  return {
    file: dotfiles ? "~/code/dotfiles/claude/settings.json" : file,
    link: dotfiles ? file : null,
    changed,
    repo: !changed ? null : dotfiles ? "dotfiles: 1 changed file, nothing committed" : file.includes("/.claude/") ? "git ignores it in shopfront" : null,
    before,
  };
}

const BROKEN = "~/code/shopfront/.claude/settings.local.json";

type Answer = { status: number; body: SkillWrite | ApiError };
const refuse = (status: number, error: string): Answer => ({ status, body: { error } });

/// `POST /api/skills/switch`, as the server checks it: every id first,
/// then one file for them all.
export function switchSkills(body: Record<string, unknown>): Answer {
  const cli = body.cli;
  const set = body.set;
  if ((cli !== "claude" && cli !== "codex" && cli !== "cursor") || !Array.isArray(set)) {
    return refuse(400, "Failed to deserialize the JSON body into the target type.");
  }
  if (set.length === 0) return refuse(400, "Name a skill to switch.");
  if (cli === "cursor") return refuse(409, "Cursor has no switch for a skill.");
  const changes: [Skill, SkillControl, SkillTo["state"]][] = [];
  for (const to of set as SkillTo[]) {
    const s = skills.find((m) => m.skill.id === to.id)?.skill;
    if (!s) return refuse(404, `There is no skill ${String(to.id)}.`);
    const c = s.controls.find((x) => x.cli === cli);
    if (!c) return refuse(409, `\`${s.name}\` has no switch for ${cli === "claude" ? "Claude" : "Codex"} the page can set.`);
    if (c.setting === "enabledPlugins") return refuse(409, `\`${s.name}\` is a plugin's skill: the plugin's switch turns it on or off.`);
    if (cli === "codex" && to.state !== "on" && to.state !== "off") return refuse(400, "Codex's switch is On or Off.");
    changes.push([s, c, to.state]);
  }
  const file = changes[0]![1].file;
  if (changes.some(([, c]) => c.file !== file)) {
    return refuse(409, "Those skills are switched in different files, so switch them one at a time.");
  }
  if (file === BROKEN) {
    return refuse(409, `${BROKEN} is not valid JSON: trailing comma at line 4 column 1, so the page did not write it.`);
  }
  const before: SkillTo[] = [];
  let changed = false;
  for (const [s, c, state] of changes) {
    before.push({ id: s.id, state: c.state });
    changed ||= c.state !== state;
    s.switches = s.switches.filter((w) => !(w.cli === c.cli && w.setting === c.setting && w.project === c.project));
    if (state !== "on") s.switches.push({ cli: c.cli, state, setting: c.setting, file: c.file, project: c.project });
    s.controls = controlsOf(s);
  }
  return { status: 200, body: written(file, changed, before) };
}

/// `POST /api/skills/plugin`.
export function switchPlugin(body: Record<string, unknown>): Answer {
  if (typeof body.plugin !== "string" || typeof body.on !== "boolean") {
    return refuse(400, "Failed to deserialize the JSON body into the target type.");
  }
  const of = skills.filter((m) => m.skill.controls.some((c) => c.plugin === body.plugin)).map((m) => m.skill);
  if (of.length === 0) return refuse(404, `There is no plugin ${body.plugin}.`);
  const changed = of.some((s) => (s.controls[0]!.state === "on") !== body.on);
  for (const s of of) {
    s.switches = s.switches.filter((w) => w.setting !== "enabledPlugins");
    if (!body.on) s.switches.push({ cli: "claude", state: "off", setting: "enabledPlugins", file: "~/.claude/settings.json", project: null });
    s.controls = controlsOf(s);
  }
  return { status: 200, body: written("~/.claude/settings.json", changed) };
}

export function skillList(): Skills {
  // Codex's built-in skill-creator switched off is no longer a name found twice.
  const builtin = skills.find((m) => m.skill.path === `${HOME}/.codex/skills/.system/skill-creator`)!.skill;
  const twice = builtin.switches.some((w) => w.cli === "codex" && w.state === "off");
  return {
    skills: [...skills.map((m) => m.skill)].sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase())),
    fixes: twice ? fixes.filter((f) => f.title !== "skill-creator twice") : fixes,
    home: HOME,
    clis: ["claude", "codex", "cursor"],
  };
}

const DAY = 86_400_000;

/// What Claude's transcripts say of the skills above: [path, uses, days
/// ago, project].
const used: [string, number, number, string | null][] = [
  [`${TOOLBOX.root}/skills/deploy`, 14, 0, "shopfront"],
  [`${HOME}/.claude/plugins/cache/tools-market/lint-kit/2.1.0/skills/lint`, 9, 2, "ledger"],
  [`${TOOLBOX.root}/skills/capture`, 6, 9, null],
  [`${SHOP.root}/.claude/skills/translate`, 4, 1, "shopfront"],
  [personal("sketchpad"), 3, 0, null],
  [`${SHOP.root}/.claude/commands/review.md`, 2, 12, "shopfront"],
  [`${HOME}/.claude/skills/synced/bucket/docs`, 1, 6, null],
  [`${DOCS.root}/.claude/skills/style-guide`, 1, 20, "docs-site"],
];

export function skillUsage(now: number): SkillUsage {
  return {
    days: 30,
    used: used.map(([path, count, ago, project]) => ({ id: id(path), count, last: now - ago * DAY - 3_600_000, project })),
  };
}

export function skillDetail(id: string): SkillDetail | null {
  const m = skills.find((x) => x.skill.id === id);
  if (!m) return null;
  const broken = m.body === null;
  const file = m.skill.kind === "command" ? m.skill.path : `${m.skill.path}/SKILL.md`;
  return {
    skill: m.skill,
    file: broken ? null : file,
    frontmatter: m.fields,
    frontmatterError: null,
    body: m.body,
    unread: broken ? "The link is broken: there is no file to read." : null,
    files: m.files.map((path) => ({ path, size: 200 + path.length * 37, link: null })),
    truncated: false,
  };
}
