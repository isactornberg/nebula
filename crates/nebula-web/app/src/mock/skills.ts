// Made-up skills for `pnpm dev`, one of each kind the page shows. Nothing
// here is a real skill or a real machine's folder.
import type {
  GitRepo,
  Skill,
  SkillDetail,
  SkillField,
  SkillLoad,
  SkillProblem,
  SkillSource,
  SkillSwitch,
  Skills,
} from "../api/contract.ts";

const HOME = "/Users/you";
const TOOLBOX: GitRepo = { root: `${HOME}/code/toolbox`, branch: "main" };
const SHOP: GitRepo = { root: `${HOME}/code/shopfront`, branch: "checkout-retries" };

interface MockSkill {
  skill: Skill;
  fields: SkillField[];
  body: string | null;
  files: string[];
}

function load(cli: SkillLoad["cli"], scope: SkillLoad["scope"], path: string, name: string, project: string | null = null): SkillLoad {
  return { cli, name, scope, path, project, space: null };
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
  mock({
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
    ],
  }),
  mock({
    name: "deploy",
    description: "Deploy the shop with its own scripts.",
    path: `${SHOP.root}/.claude/skills/deploy`,
    repo: SHOP,
    loads: [
      load("claude", "project", `${SHOP.root}/.claude/skills/deploy`, "deploy", "shopfront"),
      load("cursor", "project", `${SHOP.root}/.claude/skills/deploy`, "deploy", "shopfront"),
    ],
    problems: [
      {
        kind: "duplicateName",
        text: "Claude finds another skill named `deploy`, ~/.claude/skills/deploy (personal), and uses ~/.claude/skills/deploy (personal) instead.",
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
  mock({
    name: "old-helper",
    description: null,
    path: `${SHOP.root}/.claude/skills/old-helper`,
    repo: SHOP,
    loads: [load("claude", "project", `${SHOP.root}/.claude/skills/old-helper`, "old-helper", "shopfront")],
    problems: [
      { kind: "brokenLink", text: "The link points at ~/code/shopfront/.agents/skills/old-helper, which is not there." },
    ],
    body: null,
    files: [],
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

export function skillList(): Skills {
  return {
    skills: [...skills.map((m) => m.skill)].sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase())),
    home: HOME,
    clis: ["claude", "codex", "cursor"],
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
