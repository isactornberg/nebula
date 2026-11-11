import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Project, Skill, SkillCompare, SkillControl, SkillDone, SkillFix, SkillLoad, Skills, SkillUsage, SkillWrite } from "./api/contract";
import { parseRoute, routePath, sectionOf } from "./model";
import {
  actLabel,
  addedText,
  bestItem,
  controlsText,
  copyName,
  cureOf,
  differing,
  doneText,
  filterTree,
  groupUsed,
  placeOf,
  problemsOf,
  removeAsk,
  rowUsed,
  sessionGets,
  skillKey,
  skillsSummary,
  skillTree,
  tidyCandidates,
  tilde,
  usedLabel,
  usedText,
  usesOf,
  valueText,
  visibleItems,
  whereItLoads,
  writtenText,
  useSummary,
  type Section,
} from "./skills";
import { CompareView, DoneNote, FixRow, KeptLine, Positions, SkillPane, SkillsOverview, SkillsSide, TidyUp, Toggle, type Writes } from "./SkillsPage";

const HOME = "/home/me";

const load = (over: Partial<SkillLoad> = {}): SkillLoad => ({
  cli: "claude",
  name: "x",
  scope: "personal",
  path: `${HOME}/.claude/skills/x`,
  project: null,
  space: null,
  shadowedBy: null,
  ...over,
});

const skill = (id: string, over: Partial<Skill> = {}): Skill => ({
  id,
  name: id,
  description: null,
  kind: "skill",
  path: `${HOME}/.claude/skills/${id}`,
  source: { kind: "folder" },
  repo: null,
  loads: [load({ name: id })],
  switches: [],
  controls: [],
  problems: [],
  ...over,
});

const both = (name: string, path = `${HOME}/.claude/skills/${name}`) => [
  load({ name, path }),
  load({ cli: "cursor", name, path }),
];

const inRepo = (id: string, root: string, loads = both(id)) =>
  skill(id, { path: `${root}/skills/${id}`, source: { kind: "repo" }, repo: { root, branch: "main" }, loads });

const local = (id: string, project: string, clis: SkillLoad["cli"][] = ["claude", "cursor"]) =>
  skill(id, {
    path: `${HOME}/code/${project}/.claude/skills/${id}`,
    loads: clis.map((cli) => load({ cli, name: id, scope: "project", project, path: `${HOME}/code/${project}/.claude/skills/${id}` })),
  });

const kit = { kind: "plugin", plugin: "kit", version: "1.0.0", marketplace: "market", from: null } as const;

const all = [
  skill("zeta", { loads: both("zeta") }),
  inRepo("capture", `${HOME}/code/toolbox`),
  inRepo("changelog", `${HOME}/code/toolbox`, [
    load({ cli: "codex", name: "changelog", path: `${HOME}/.agents/skills/changelog` }),
    load({ cli: "cursor", name: "changelog", path: `${HOME}/.agents/skills/changelog` }),
  ]),
  inRepo("alpha", `${HOME}/code/toolbox`),
  skill("lint", { source: kit, loads: [load({ name: "kit:lint", scope: "plugin" })] }),
  skill("tidy", {
    source: { kind: "plugin", plugin: "neat", version: "a1b2c3d4e5f60718", marketplace: "market", from: null },
    loads: [load({ name: "neat:tidy", scope: "plugin" })],
    switches: [{ cli: "claude", state: "off", setting: "enabledPlugins", file: null, project: null }],
  }),
  skill("imagegen", { source: { kind: "bundled", cli: "codex" }, loads: [load({ cli: "codex", scope: "system" })] }),
  skill("docs", { source: { kind: "claudeAi" } }),
  skill("watch", { source: { kind: "installer", source: "acme/video", url: null, path: null } }),
  local("translate", "shop"),
  local("deploy", "shop"),
  local("flash", "rover", ["codex", "cursor"]),
  skill("vendor-api", {
    path: `${HOME}/code/ledger/.claude/skills/vendor-api`,
    loads: [load({ name: "vendor-api", scope: "project", project: "ledger" })],
    problems: [{ kind: "brokenLink", text: "The link points at a folder that is not there.", kept: false }],
  }),
  skill("style", {
    loads: [
      load({ name: "style", scope: "project", project: "docs" }),
      load({ name: "style", scope: "addedDir", space: "Acme" }),
    ],
  }),
  skill("onboarding", { loads: [load({ name: "onboarding", scope: "addedDir", space: "Acme" })] }),
];

const PROJECTS: Project[] = [
  { id: "p1", name: "docs", space: null },
  { id: "p2", name: "shop", space: "Acme" },
  { id: "p3", name: "rover", space: "Acme" },
  { id: "p4", name: "ledger", space: null },
];

const tree = (skills = all, cli: SkillLoad["cli"] | null = null) => skillTree(skills, HOME, PROJECTS, cli);

/// Each section's groups, as title, where-line and the skills in it, with
/// the CLIs of a row whose CLIs are not its group's.
const outline = (sections: Section[]) =>
  sections.map((sec) =>
    sec.groups.map((g) => [
      g.title,
      g.note,
      [...g.children.map((c) => `${c.title} (${c.note})`), ...g.rows.map((r) => (r.cliNote ? `${r.skill.name}: ${r.cliNote}` : r.skill.name))],
    ]),
  );

const key = (k: string) => ({ key: k, metaKey: false, ctrlKey: false, altKey: false });

describe("skills routes", () => {
  it("parses the list and a skill, and round-trips an id", () => {
    expect(parseRoute("/skills")).toEqual({ kind: "skills" });
    expect(parseRoute("/skills/")).toEqual({ kind: "skills" });
    expect(parseRoute("/skills/abc123")).toEqual({ kind: "skill", id: "abc123" });
    expect(parseRoute("/skills/fixes")).toEqual({ kind: "skillFixes" });
    expect(routePath({ kind: "skillFixes" })).toBe("/skills/fixes");
    expect(parseRoute("/skills/a/b")).toEqual({ kind: "idle" });
    expect(parseRoute("/skillsx")).toEqual({ kind: "idle" });
    const r = { kind: "skill", id: "a b/c" } as const;
    expect(parseRoute(routePath(r))).toEqual(r);
    expect(routePath({ kind: "skills" })).toBe("/skills");
  });

  it("puts each route in its section", () => {
    expect(sectionOf({ kind: "idle" })).toBe("attention");
    expect(sectionOf({ kind: "session", id: "a" })).toBe("attention");
    expect(sectionOf({ kind: "skills" })).toBe("skills");
    expect(sectionOf({ kind: "skill", id: "a" })).toBe("skills");
    expect(sectionOf({ kind: "skillFixes" })).toBe("skills");
  });
});

describe("the skills tree", () => {
  it("puts what loads everywhere under GLOBAL by where it comes from, and the rest under its project", () => {
    expect(outline(tree())).toEqual([
      [
        ["toolbox", "~/code/toolbox · main", ["alpha", "capture", "changelog: Codex and Cursor"]],
        ["Plain folders", "in no repository", ["zeta"]],
        ["npx skills", "from 1 GitHub repo", ["watch"]],
        ["Plugins", "Claude only · 1 on, 1 off", ["kit (market · 1.0.0)", "neat (market · a1b2c3d · off)"]],
        ["claude.ai", "synced from your account", ["docs"]],
        ["Built in", "Codex 1", ["imagegen"]],
      ],
      [
        ["docs", "1 also in space Acme", ["style"]],
        ["ledger", "all broken links", ["vendor-api"]],
        ["shop", "", ["deploy", "translate"]],
        ["rover", "", ["flash"]],
        ["Added directories", "all its projects", ["onboarding"]],
      ],
    ]);
    const shop = tree()[1]!.groups.find((g) => g.title === "shop")!;
    expect(shop.space).toBe("Acme");
  });

  it("puts a skill loaded everywhere by one CLI and in a project by another in both, each with its own CLIs", () => {
    const split = skill("release", {
      loads: [
        load({ name: "release" }),
        load({ cli: "codex", name: "release", scope: "project", project: "shop", path: `${HOME}/code/shop/.agents/skills/release` }),
        load({ name: "release", scope: "project", project: "shop", path: `${HOME}/code/shop/.claude/skills/release` }),
      ],
    });
    const [global, projects] = tree([split, local("deploy", "shop")]);
    expect(global!.groups.flatMap((g) => g.rows.map((r) => [r.skill.name, r.clis]))).toEqual([["release", ["claude"]]]);
    expect(projects!.groups[0]!.rows.map((r) => [r.skill.name, r.clis, r.cliNote])).toEqual([
      ["deploy", ["claude", "cursor"], null],
      ["release", ["codex"], "Codex only"],
    ]);
    const linked = skill("notes", { loads: [load({ name: "notes" }), load({ name: "notes", scope: "project", project: "shop" })] });
    expect(outline(tree([linked]))[1]).toEqual([]);
  });

  it("puts the copy of a name Claude loads first, then Codex's, then Cursor's", () => {
    const copy = (id: string, path: string, clis: SkillLoad["cli"][]) =>
      inRepo(id, `${HOME}/code/toolbox`, clis.map((cli) => load({ cli, name: "capture", path })));
    const copies = [
      copy("a-cursor", `${HOME}/.cursor/skills/capture`, ["cursor"]),
      copy("b-codex", `${HOME}/.agents/skills/capture`, ["codex", "cursor"]),
      copy("c-claude", `${HOME}/.claude/skills/capture`, ["claude", "cursor"]),
      copy("d-claude", `${HOME}/.claude/skills/capture`, ["claude"]),
    ].map((s) => ({ ...s, name: "capture" }));
    const rows = tree(copies)[0]!.groups[0]!.rows.map((r) => r.skill.id);
    expect(rows).toEqual(["c-claude", "d-claude", "b-codex", "a-cursor"]);
  });

  it("puts a skill two projects load under each", () => {
    const shared = skill("delegate", {
      loads: [load({ name: "delegate", scope: "project", project: "shop" }), load({ name: "delegate", scope: "project", project: "docs" })],
    });
    expect(outline(tree([shared]))[1]!.map((g) => g[0])).toEqual(["docs", "shop"]);
  });

  it("keeps what one CLI loads when one is picked", () => {
    expect(outline(tree(all, "codex"))).toEqual([
      [
        ["toolbox", "~/code/toolbox · main", ["changelog"]],
        ["Built in", "Codex 1", ["imagegen"]],
      ],
      [["rover", "", ["flash"]]],
    ]);
  });

  it("filters by every word, on names, descriptions and groups, and keeps the groups left", () => {
    const names = (q: string) => outline(filterTree(tree(), q)).map((sec) => sec.flatMap((g) => g[2]));
    expect(names("CAP")).toEqual([["capture"], []]);
    expect(names("kit:")).toEqual([["kit (market · 1.0.0)"], []]);
    expect(names("toolbox a")).toEqual([["alpha", "capture", "changelog: Codex and Cursor"], []]);
    expect(names("shop")).toEqual([[], ["deploy", "translate"]]);
    expect(names("nothing here")).toEqual([[], []]);
    const described = [skill("ship", { description: "Deploys the branch to staging" })];
    expect(outline(filterTree(tree(described), "staging"))[0]![0]![2]).toEqual(["ship"]);
  });

  it("lists the lines on screen, the open groups unfolded, and finds where a skill is", () => {
    const sections = tree();
    const keys = (open: string[]) => visibleItems(sections, new Set(open)).map((i) => i.key);
    expect(keys([])).toHaveLength(11);
    expect(keys(["plugins", "plugin:kit@market"]).slice(3, 7)).toEqual(["plugins", "plugin:kit@market", "plugin:kit@market/lint", "plugin:neat@market"]);
    expect(placeOf(sections, "lint")?.path.map((g) => g.title)).toEqual(["Plugins", "kit"]);
    expect(placeOf(sections, "deploy")).toMatchObject({ section: "projects" });
    const items = visibleItems(filterTree(sections, "a"), new Set(["repo:/home/me/code/toolbox"]));
    expect(bestItem(items, "a")?.key).toBe("repo:/home/me/code/toolbox/alpha");
  });

  it("says what a session in a project gets from each CLI", () => {
    const shadowed = skill("deploy", {
      loads: [load({ name: "deploy", scope: "project", project: "shop", shadowedBy: "zeta" })],
    });
    const skills = [...all, shadowed];
    // Six everywhere (not neat, which is off), the shop's two (not the
    // deploy another one wins over), and its space's two.
    expect(sessionGets(skills, "shop", "acme", "claude")).toBe(10);
    expect(sessionGets(skills, "rover", "Acme", "codex")).toBe(3);
    expect(sessionGets(skills, "ledger", null, "claude"), "not its broken link").toBe(6);
  });

  it("sums up the tree in one line", () => {
    expect(skillsSummary(tree(), all, ["claude", "codex", "cursor"], 2)).toBe(
      "15 skills for Claude, Codex and Cursor: 9 load in every project, 6 in one of 4 projects. 2 things need fixing.",
    );
    expect(skillsSummary(tree([skill("zeta")]), [skill("zeta")], ["claude"], 0)).toBe(
      "1 skill for Claude, all of them in every project. Nothing needs fixing.",
    );
  });
});

describe("where a skill loads", () => {
  const winner = skill("deploy", { loads: both("deploy") });
  const loser = skill("deploy-shop", {
    name: "deploy",
    loads: [
      load({ name: "deploy", scope: "project", project: "shop", path: "/code/shop/.claude/skills/deploy", shadowedBy: "deploy" }),
      load({ cli: "cursor", name: "deploy", scope: "project", project: "shop", path: "/code/shop/.claude/skills/deploy" }),
    ],
    switches: [{ cli: "claude", state: "nameOnly", setting: "skillOverrides", file: null, project: "shop" }],
  });
  const codexOwn = skill("deploy-codex", {
    name: "deploy",
    loads: [load({ cli: "codex", name: "deploy", path: `${HOME}/.codex/skills/deploy` })],
  });
  const list = [winner, loser, codexOwn];
  const where = (s: Skill, cli: SkillLoad["cli"]) => {
    const w = whereItLoads(s, cli, list, HOME);
    return [w.where, w.why, w.instead?.id ?? null, w.state];
  };

  it("says everywhere, or the projects, and its switch", () => {
    expect(where(winner, "claude")).toEqual(["everywhere", null, null, "On"]);
    expect(where(winner, "cursor")).toEqual(["everywhere", null, null, null]);
    expect(where(loser, "cursor")).toEqual(["shop", null, null, null]);
  });

  it("says no, and why, when another skill of the name wins, or the CLI reads elsewhere", () => {
    expect(where(loser, "claude")).toEqual([null, "Claude loads another `deploy`, from ~/.claude/skills", "deploy", null]);
    expect(where(winner, "codex")).toEqual([null, "Codex loads another `deploy`, from ~/.codex/skills", "deploy-codex", null]);
    expect(where(codexOwn, "claude")[0]).toBe(null);
    expect(where(skill("solo", { loads: [] }), "codex")[1]).toBe(
      "Codex reads ~/.agents/skills, ~/.codex/skills and a project's .agents/skills",
    );
    expect(whereItLoads(all[4]!, "codex", all, HOME).why).toBe("a plugin is Claude's");
  });

  it("says a switch in a project beside the one everywhere", () => {
    const both = skill("x", {
      switches: [
        { cli: "claude", state: "off", setting: "skillOverrides", file: null, project: null },
        { cli: "claude", state: "nameOnly", setting: "skillOverrides", file: null, project: "shop" },
      ],
    });
    expect(whereItLoads(both, "claude", [both], HOME).state).toBe("Off, name only in shop");
  });
});

describe("the skills page", () => {
  const got = { ok: true as const, value: { skills: all, fixes: [], kept: [], home: HOME, clis: ["claude", "codex", "cursor"] } as Skills };
  const side = (props: Partial<Parameters<typeof SkillsSide>[0]> = {}) =>
    renderToStaticMarkup(
      <SkillsSide
        got={got}
        sections={tree()}
        open={new Set()}
        cursor={null}
        openId={null}
        query=""
        cli={null}
        filter={{ current: null }}
        gets={(g) => (g.project ? `A session here gets ${g.project}.` : null)}
        onQuery={() => {}}
        onFilterKey={() => {}}
        onCli={() => {}}
        onFold={() => {}}
        onOpen={() => {}}
        onFixes={() => {}}
        {...props}
      />,
    );

  it("draws every group folded, with its count, where-line and problems", () => {
    const html = side();
    expect(html).toContain('<h2 id="skills-global" class="caps">Global 9</h2>');
    expect(html).toContain('<h2 id="skills-projects" class="caps">Projects 6</h2>');
    expect(html).toMatch(/aria-expanded="false"><svg[^]*?<span class="g-title">toolbox<\/span><span class="g-count">3<\/span><span class="g-note">~\/code\/toolbox · main<\/span><\/button>/);
    expect(html).toContain('<span class="g-problems"><span aria-hidden="true" class="dot tone-red"></span>1<span class="sr"> with a problem</span></span>');
    expect(html).toContain('<p class="space-label">Acme</p>');
    expect(html).not.toContain("fixes-row");
    expect(html).not.toContain("skill-row");
    expect(html).toContain('<button type="button" aria-pressed="true">All 15</button>');
    expect(html).toContain('<button type="button" aria-pressed="false">Codex 3</button>');
  });

  it("draws an open group's skills, quiet but for what is unusual, and a project's session line", () => {
    const html = side({
      open: new Set(["repo:/home/me/code/toolbox", "project:shop"]),
      cursor: "repo:/home/me/code/toolbox/alpha",
      openId: "capture",
    });
    expect(html).toContain(
      '<a class="row skill-row" data-depth="1" data-cursor="true" href="/skills/alpha"><span class="title">alpha</span></a>',
    );
    expect(html).toContain('aria-current="page" href="/skills/capture"');
    expect(html).toContain('<span class="title">changelog</span><span class="s-clis">Codex and Cursor</span>');
    expect(html).toContain('<p class="gets" data-depth="1">A session here gets shop.</p>');
    expect(html).not.toContain("desc");
  });

  it("leads to what needs fixing from the list, which a phone shows alone", () => {
    const fix: SkillFix = { id: "f1", kind: "oneCli", title: "zeta", place: "for Claude only", text: "", fix: "", skills: ["zeta"], acts: [] };
    const html = side({ got: { ok: true, value: { ...got.value, fixes: [fix, fix] } } });
    expect(html).toContain(
      '<a class="row fixes-row" href="/skills/fixes"><span aria-hidden="true" class="dot tone-red"></span><span class="title">2 things need fixing</span><span class="more" aria-hidden="true">›</span></a>',
    );
  });

  it("says so when a filter leaves nothing", () => {
    expect(side({ sections: filterTree(tree(), "zzz"), query: "zzz" })).toContain("No skill matches the filter.");
  });

  it("lists one row per fix, the ones about several skills with a toggle for them", () => {
    const fixes: SkillFix[] = [
      {
        id: "f1",
        kind: "brokenLink",
        title: "2 broken links",
        place: "in ledger",
        text: "2 links in .claude/skills point into .agents/skills, which is not there.",
        fix: "Remove the links, or put back .agents/skills.",
        skills: ["vendor-api", "zeta"],
        acts: [],
      },
      { id: "f2", kind: "nameMismatch", title: "zeta", place: "in ~/.claude/skills", text: "The name is `Zeta`.", fix: "Rename it.", skills: ["zeta"], acts: [] },
    ];
    const list = { ...got.value, fixes, skills: all.map((s) => (s.id === "zeta" ? { ...s, problems: [{ kind: fixes[1]!.kind, text: fixes[1]!.text, kept: false }] } : s)) };
    const html = renderToStaticMarkup(<SkillsOverview list={list} tree={tree()} note={null} onBack={null} onOpen={() => {}} />);
    expect(html).toContain("Needs fixing 2");
    expect(html).toContain("2 problems, one row per fix");
    expect(html).toContain('<span class="fix-title">2 broken links</span><span class="muted"> in ledger</span>');
    expect(html).toContain('<button type="button" class="fix-act" aria-expanded="false">Show them</button>');
    expect(html).toContain('<a class="fix-act" href="/skills/zeta">Open it ›</a>');
    expect(html).toContain("<code>Zeta</code>");
    expect(html).not.toContain("‹ Skills");
    const alone = renderToStaticMarkup(<SkillsOverview list={list} tree={tree()} note={null} onBack={() => {}} onOpen={() => {}} />);
    expect(alone).toContain('<a class="back" href="/skills">‹ Skills</a>');
  });
});

describe("skills keys", () => {
  it("moves, folds, opens and goes back, and / goes to the filter", () => {
    expect(skillKey(key("j"), false, "")).toEqual({ kind: "move", delta: 1 });
    expect(skillKey(key("ArrowUp"), false, "")).toEqual({ kind: "move", delta: -1 });
    expect(skillKey(key("ArrowLeft"), false, "")).toEqual({ kind: "fold", open: false });
    expect(skillKey(key("l"), false, "")).toEqual({ kind: "fold", open: true });
    expect(skillKey(key("Enter"), false, "")).toEqual({ kind: "open" });
    expect(skillKey(key("Escape"), false, "")).toEqual({ kind: "back" });
    expect(skillKey(key("/"), false, "")).toEqual({ kind: "filter" });
    expect(skillKey(key("t"), false, "")).toBe(null);
    expect(skillKey(key("1"), false, "")).toBe(null);
    expect(skillKey({ ...key("/"), metaKey: true }, false, "")).toBe(null);
  });

  it("types letters in the filter, which still moves and opens, and clears it before leaving", () => {
    expect(skillKey(key("j"), true, "")).toBe(null);
    expect(skillKey(key("ArrowLeft"), true, "")).toBe(null);
    expect(skillKey(key("ArrowDown"), true, "")).toEqual({ kind: "move", delta: 1 });
    expect(skillKey(key("Enter"), true, "x")).toEqual({ kind: "open" });
    expect(skillKey(key("Escape"), true, "x")).toEqual({ kind: "clear" });
    expect(skillKey(key("Escape"), true, "")).toEqual({ kind: "leave" });
  });
});

describe("skills text", () => {
  it("shows paths under the home as ~", () => {
    expect(tilde(`${HOME}/code`, HOME)).toBe("~/code");
    expect(tilde(HOME, HOME)).toBe("~");
    expect(tilde("/home/meadow/x", HOME)).toBe("/home/meadow/x");
  });

  it("writes a frontmatter value as text", () => {
    expect(valueText("hi")).toBe("hi");
    expect(valueText(true)).toBe("true");
    expect(valueText(["Bash(git *)", "Read"])).toBe("Bash(git *), Read");
    expect(valueText({ owner: "ops", tags: ["a", "b"] })).toBe("owner: ops\ntags: a, b");
    expect(valueText({ hooks: { PreToolUse: [{ matcher: "Bash" }] } })).toBe(
      "hooks:\n  PreToolUse:\n    - matcher: Bash",
    );
  });
});

describe("skill use", () => {
  const now = new Date(2026, 10, 11, 15, 0).getTime();
  const at = (day: number, hour = 9) => new Date(2026, 10, day, hour, 0).getTime();
  const usage: SkillUsage = {
    days: 30,
    used: [
      { id: "capture", count: 5, last: at(11), project: null },
      { id: "lint", count: 3, last: at(9), project: "shop" },
      { id: "translate", count: 2, last: at(10, 23), project: "shop" },
      { id: "tidy", count: 1, last: at(1), project: null },
      { id: "zeta", count: 1, last: at(2), project: "docs" },
      { id: "docs", count: 1, last: at(2), project: null },
      { id: "deploy", count: 1, last: at(1), project: "shop" },
    ],
  };
  const uses = usesOf(usage);
  const list: Skills = { skills: all, fixes: [], kept: [], home: HOME, clis: ["claude", "codex", "cursor"] };

  it("says how many days ago by the calendar", () => {
    expect(usedLabel(at(11, 1), now)).toBe("today");
    expect(usedLabel(at(10, 23), now)).toBe("1d");
    expect(usedLabel(at(10, 23), at(11, 0))).toBe("1d");
    expect(usedLabel(new Date(2026, 9, 28, 12).getTime(), now)).toBe("14d");
  });

  it("puts the time on a row Claude loads its skill on, and a group's from its latest", () => {
    const [global, local] = tree();
    const toolbox = global!.groups.find((g) => g.title === "toolbox")!;
    const shop = local!.groups.find((g) => g.project === "shop")!;
    // Codex and Cursor load changelog, whose use is not read.
    const andCodex = usesOf({ ...usage, used: [...usage.used, { id: "changelog", count: 1, last: at(11), project: null }] });
    expect(toolbox.rows.map((r) => [r.skill.name, rowUsed(r, andCodex)])).toEqual([
      ["alpha", null],
      ["capture", at(11)],
      ["changelog", null],
    ]);
    expect(groupUsed(toolbox, uses)).toBe(at(11));
    expect(groupUsed(shop, uses)).toBe(at(10, 23));
    expect(groupUsed(local!.groups.find((g) => g.project === "rover")!, uses)).toBe(null);
  });

  it("draws the time in its own column, kept when there is none", () => {
    const html = renderToStaticMarkup(
      <SkillsSide
        got={{ ok: true, value: list }}
        sections={tree()}
        open={new Set(["repo:/home/me/code/toolbox"])}
        cursor={null}
        openId={null}
        query=""
        cli={null}
        filter={{ current: null }}
        gets={() => null}
        used={{ uses, now }}
        onQuery={() => {}}
        onFilterKey={() => {}}
        onCli={() => {}}
        onFold={() => {}}
        onOpen={() => {}}
        onFixes={() => {}}
      />,
    );
    expect(html).toContain(
      '<span class="g-note">~/code/toolbox · main</span><span class="used"><span class="sr">, used </span>today</span></button>',
    );
    expect(html).toContain('<span class="title">alpha</span><span class="used"></span></a>');
    expect(html).toContain('<span class="g-title">shop</span><span class="g-count">2</span><span class="g-note"></span><span class="used"><span class="sr">, used </span>1d</span>');
  });

  it("says on a skill's page how often, when last and where, or why not", () => {
    const byId = (id: string) => all.find((s) => s.id === id)!;
    expect(usedText(byId("capture"), usage, uses)).toEqual({ text: "5 times in 30 days, last on 11 Nov", quiet: false });
    expect(usedText(byId("deploy"), usage, uses)).toEqual({ text: "once in 30 days, on 1 Nov in shop", quiet: false });
    expect(usedText(byId("alpha"), usage, uses)).toEqual({ text: "not in 30 days", quiet: true });
    expect(usedText(byId("flash"), usage, uses)).toEqual({ text: "not read for Codex and Cursor", quiet: true });
  });

  it("lists the most used under what needs fixing, and in each place how many Claude loads were not", () => {
    const overview = (u: Parameters<typeof SkillsOverview>[0]["usage"]) =>
      renderToStaticMarkup(<SkillsOverview list={list} tree={tree()} usage={u} uses={uses} note={null} onBack={null} onOpen={() => {}} />);
    const html = overview({ ok: true, value: usage });
    expect(html).toContain('<h2 id="use-title" class="caps">Use, last 30 days</h2>');
    expect(html).toContain("from Claude Code transcripts; Codex and Cursor are not read");
    expect(html).toContain("Most used · 7 skills in all");
    expect(html).toContain(
      '<a class="use-row" href="/skills/lint"><span class="u-name">kit:lint</span><span class="u-where">plugin</span><span class="u-count">3</span></a>',
    );
    expect(html).toContain('<span class="u-name">translate</span><span class="u-where">shop</span>');
    expect(html).toContain('<span class="u-name">capture</span><span class="u-where">global</span>');
    expect(html).not.toContain("/skills/deploy");
    expect(html).toContain('<button type="button" class="use-more" aria-expanded="false">All 7 by use ›</button>');
    expect(html).toContain("Not used · 4 that Claude loads");
    expect(html).toContain('<li class="use-row"><span class="u-name">Yours, global</span><span class="u-of">2 of 4</span></li>');
    expect(html).toContain('<span class="u-name">Projects</span><span class="u-of">2 of 4</span>');
    expect(html).not.toContain("claude.ai</span>");
    expect(html).not.toContain("kit plugin");
    expect(html).toContain("Not counted: 1 switched off, 1 built in, 1 broken link and 2 of yours that only Codex and Cursor load.");
    expect(overview(null)).toContain("Reading the transcripts…");
    expect(overview({ ok: false, error: "Reading Claude's transcripts failed." })).toContain(
      '<p class="error">Reading Claude&#x27;s transcripts failed.</p>',
    );
    const noClaude = renderToStaticMarkup(
      <SkillsOverview list={{ ...list, clis: ["codex"] }} tree={tree()} usage={null} note={null} onBack={null} onOpen={() => {}} />,
    );
    expect(noClaude).not.toContain("use-title");
  });
});

describe("the switches", () => {
  const control = (over: Partial<SkillControl> = {}): SkillControl => ({
    cli: "claude",
    setting: "skillOverrides",
    file: "~/.claude/settings.json",
    state: "on",
    plugin: null,
    project: null,
    ...over,
  });
  const writes = (over: Partial<Writes> = {}): Writes => ({ busy: null, said: null, write: () => {}, act: () => {}, asking: null, ask: () => {}, ...over });

  it("draws Claude's four positions and Codex's two, the one it is at pressed", () => {
    const four = renderToStaticMarkup(
      <Positions control={control({ state: "nameOnly" })} label="capture, for Claude" busy={false} onPick={() => {}} />,
    );
    expect(four).toBe(
      '<div role="group" aria-label="capture, for Claude" class="positions">' +
        '<button type="button" aria-pressed="false">On</button>' +
        '<button type="button" aria-pressed="true">Name only</button>' +
        '<button type="button" aria-pressed="false">Slash menu only</button>' +
        '<button type="button" aria-pressed="false">Off</button></div>',
    );
    const two = renderToStaticMarkup(
      <Positions control={control({ cli: "codex", setting: "skills.config", state: "off" })} label="x" busy onPick={() => {}} />,
    );
    expect(two).toContain('aria-busy="true"');
    expect(two.match(/<button/g)).toHaveLength(2);
    expect(renderToStaticMarkup(<Toggle on label="kit plugin, for Claude" busy={false} onFlip={() => {}} />)).toContain(
      'role="switch" class="toggle" aria-checked="true" aria-label="kit plugin, for Claude"',
    );
    expect(renderToStaticMarkup(<Toggle on label="x" busy onFlip={() => {}} />), "the flip it is making").toContain('aria-checked="false"');
  });

  it("says which file each switch writes, that it keeps the rest, and when sessions go by it", () => {
    const one = skill("capture", { controls: [control()] });
    expect(controlsText(one)).toEqual({
      writes: "A switch writes `skillOverrides` in ~/.claude/settings.json and keeps every other key.",
      when: "Claude sessions go by it from their next turn.",
    });
    const both = skill("capture", { controls: [control(), control({ cli: "codex", setting: "skills.config", file: "~/.codex/config.toml" })] });
    expect(controlsText(both)).toEqual({
      writes:
        "Claude's switch writes `skillOverrides` in ~/.claude/settings.json, Codex's `skills.config` in ~/.codex/config.toml, and each keeps every other key.",
      when: "Claude sessions go by it from their next turn; Codex sessions go by it once restarted.",
    });
    const plugin = skill("lint", { source: kit, controls: [control({ setting: "enabledPlugins", plugin: "kit@market" })] });
    expect(controlsText(plugin)?.writes).toBe(
      "A switch writes `enabledPlugins` in ~/.claude/settings.json for the whole `kit` plugin and keeps every other key.",
    );
    expect(controlsText(plugin)?.when).toBe("New Claude sessions load it, a running one after /reload-plugins.");
    expect(controlsText(skill("x"))).toBe(null);
  });

  it("says what a write wrote, and how its repository stands", () => {
    const w: SkillWrite = { file: "~/code/dotfiles/settings.json", link: "~/.claude/settings.json", changed: true, repo: "dotfiles: 1 changed file, nothing committed", before: [] };
    expect(writtenText(w)).toBe("Wrote ~/code/dotfiles/settings.json, which ~/.claude/settings.json links to · dotfiles: 1 changed file, nothing committed");
    expect(writtenText({ ...w, link: null, repo: null })).toBe("Wrote ~/code/dotfiles/settings.json");
    expect(writtenText({ ...w, changed: false })).toBe("~/.claude/settings.json says so already.");
  });

  it("draws a switch on each plugin's row, and none without a way to write", () => {
    const plugins = [
      skill("lint", { source: kit, loads: [load({ name: "kit:lint", scope: "plugin" })], controls: [control({ setting: "enabledPlugins", plugin: "kit@market" })] }),
      skill("tidy", {
        source: { kind: "plugin", plugin: "neat", version: "1", marketplace: "market", from: null },
        loads: [load({ name: "neat:tidy", scope: "plugin" })],
        switches: [{ cli: "claude", state: "off", setting: "enabledPlugins", file: null, project: null }],
        controls: [control({ setting: "enabledPlugins", plugin: "neat@market", state: "off" })],
      }),
    ];
    const side = (w: Writes | null) =>
      renderToStaticMarkup(
        <SkillsSide
          got={{ ok: true, value: { skills: plugins, fixes: [], kept: [], home: HOME, clis: ["claude"] } }}
          sections={tree(plugins)}
          open={new Set(["plugins"])}
          cursor={null}
          openId={null}
          query=""
          cli={null}
          filter={{ current: null }}
          gets={() => null}
          onQuery={() => {}}
          onFilterKey={() => {}}
          onCli={() => {}}
          onFold={() => {}}
          onOpen={() => {}}
          onFixes={() => {}}
          writes={w}
        />,
      );
    const html = side(writes({ said: { at: "plugin:neat@market", text: "Wrote ~/.claude/settings.json", error: false } }));
    expect(html).toContain('<div class="row group-row has-switch" data-depth="1"><button type="button" class="fold" aria-expanded="false">');
    expect(html).toContain('aria-checked="true" aria-label="kit plugin, for Claude"');
    expect(html).toContain('aria-checked="false" aria-label="neat plugin, for Claude"');
    expect(html).toContain('<p class="gets said" data-depth="2" role="status">Wrote ~/.claude/settings.json</p>');
    expect(side(null)).not.toContain('role="switch"');
  });

  it("offers to turn off Codex's built-in where it is one of a name found twice", () => {
    const builtin = skill("creator-codex", {
      name: "skill-creator",
      source: { kind: "bundled", cli: "codex" },
      loads: [load({ cli: "codex", scope: "system", name: "skill-creator" })],
      controls: [control({ cli: "codex", setting: "skills.config", file: "~/.codex/config.toml" })],
    });
    const fix: SkillFix = {
      id: "f9",
      kind: "duplicateName",
      title: "skill-creator twice",
      place: "in Codex",
      text: "",
      fix: "",
      skills: ["zeta", "creator-codex"],
      acts: [{ act: "keep", says: null, refused: null }],
    };
    expect(cureOf(fix, [...all, builtin])).toMatchObject({ label: "Turn off Codex's built-in", cli: "codex", to: { id: "creator-codex", state: "off" } });
    const off = { ...builtin, controls: [control({ cli: "codex", setting: "skills.config", state: "off" })] };
    expect(cureOf(fix, [...all, off]), "off already").toBe(null);
    expect(cureOf({ ...fix, kind: "brokenLink" }, [...all, builtin])).toBe(null);
    const list = { skills: [...all, builtin], fixes: [fix], kept: [], home: HOME, clis: ["claude", "codex", "cursor"] } as Skills;
    const html = renderToStaticMarkup(<SkillsOverview list={list} tree={tree(list.skills)} note={null} writes={writes()} onBack={null} onOpen={() => {}} />);
    expect(html).toContain(
      '<div class="fix-acts"><button type="button" class="btn">Turn off Codex&#x27;s built-in</button><button type="button" class="fix-act">Keep both</button>',
    );
    const done = renderToStaticMarkup(
      <SkillsOverview
        list={list}
        tree={tree(list.skills)}
        note={null}
        writes={writes({ said: { at: "fix:f9", text: "Turned off it. Wrote ~/.codex/config.toml", error: false } })}
        onBack={null}
        onOpen={() => {}}
      />,
    );
    expect(done).toContain('<div class="note done" role="status"><div class="done-text"><p>Turned off it. Wrote ~/.codex/config.toml</p></div></div>');
    const none = renderToStaticMarkup(<SkillsOverview list={list} tree={tree(list.skills)} note={null} onBack={null} onOpen={() => {}} />);
    expect(none).not.toContain("Turn off");
  });
});

describe("the tidy-up", () => {
  const on = (s: Skill): Skill => ({
    ...s,
    controls: [{ cli: "claude", setting: "skillOverrides", file: "~/.claude/settings.json", state: "on", plugin: null, project: null }],
  });
  const skills = all.map((s) => (["zeta", "capture", "alpha", "docs", "translate"].includes(s.id) ? on(s) : s));
  const slashOnly = { ...on(skill("quiet")), controls: [{ ...on(skill("q")).controls[0]!, state: "userInvocableOnly" as const }] };
  const uses = usesOf({ days: 30, used: [{ id: "capture", count: 1, last: 0, project: null }] });

  it("makes the user's own skills that load everywhere and went unused Slash menu only, and nothing else", () => {
    const names = tidyCandidates(tree([...skills, slashOnly]), uses).map((s) => s.name);
    // Not capture, used; not docs, synced; not translate, a project's;
    // not quiet, Slash menu only already; not the plugins' or built-ins.
    expect(names).toEqual(["alpha", "zeta"]);
  });

  it("names every skill it will change before it does, and offers Undo after", () => {
    const writes: Writes = { busy: null, said: null, write: () => {}, act: () => {}, asking: null, ask: () => {} };
    const candidates = tidyCandidates(tree(skills), uses);
    const before = renderToStaticMarkup(<TidyUp candidates={candidates} days={30} writes={writes} tidied={null} onTidied={() => {}} />);
    expect(before).toContain('<h2 id="tidy-title" class="caps">Tidy up</h2>');
    expect(before).toContain('<h3 class="use-head">Yours, global · 2 not used</h3><p class="tidy-names">alpha and zeta</p>');
    expect(before).toContain('<button type="button" class="btn">Make the 2 slash menu only</button>');
    const tidied = { names: ["alpha", "zeta"], before: [] };
    const said = { at: "tidy", text: "Wrote ~/.claude/settings.json · dotfiles: 1 changed file, nothing committed", error: false };
    const after = renderToStaticMarkup(<TidyUp candidates={[]} days={30} writes={{ ...writes, said }} tidied={tidied} onTidied={() => {}} />);
    expect(after).toContain("<p>Made the 2 slash menu only: alpha and zeta.</p>");
    expect(after).toContain('<button type="button" class="btn">Undo</button>');
    expect(after).toContain('<p class="said" role="status">Wrote ~/.claude/settings.json · <span class="together">dotfiles: 1 changed file, nothing committed</span></p>');
    const failed = renderToStaticMarkup(
      <TidyUp candidates={candidates} days={30} writes={{ ...writes, said: { at: "tidy", text: "It changed.", error: true } }} tidied={null} onTidied={() => {}} />,
    );
    expect(failed).toContain('<p class="error" role="alert">It changed.</p>');
    expect(renderToStaticMarkup(<TidyUp candidates={[]} days={30} writes={writes} tidied={null} onTidied={() => {}} />)).toBe("");
  });
});

describe("places in few words", () => {
  it("counts the spaces a project's skills are also in, and says them in full when it is open", () => {
    const docs = tree()[1]!.groups.find((g) => g.project === "docs")!;
    expect(docs.note).toBe("1 also in space Acme");
    expect(addedText(docs)).toBe("1 of them is also in every project of space Acme, through an added directory.");
    const twice = skill("style2", {
      loads: [
        load({ name: "style2", scope: "project", project: "docs" }),
        load({ name: "style2", scope: "addedDir", space: "Acme" }),
        load({ name: "style2", scope: "addedDir", space: "Home" }),
      ],
    });
    const group = tree([...all, twice])[1]!.groups.find((g) => g.project === "docs")!;
    expect(group.note).toBe("2 also in 2 spaces");
    expect(addedText(group)).toBe("2 of them are also in every project of spaces Acme and Home, through an added directory.");
  });

  it("puts a skill that loads in several places in the use list as the first and how many more", () => {
    const style = all.find((s) => s.id === "style")!;
    const sum = useSummary(tree(), all, ["claude"], { days: 30, used: [{ id: "style", count: 2, last: 0, project: "docs" }] }, usesOf(null));
    expect(sum.most.find((m) => m.skill === style)?.where).toBe("docs +1");
  });
});

describe("the fixes' acts", () => {
  const writes = (over: Partial<Writes> = {}): Writes => ({ busy: null, said: null, write: () => {}, act: () => {}, asking: null, ask: () => {}, ...over });
  const act = (a: SkillFix["acts"][number]["act"], more: Partial<SkillFix["acts"][number]> = {}) => ({ act: a, says: null, refused: null, ...more });
  const links: SkillFix = {
    id: "f1",
    kind: "brokenLink",
    title: "2 broken links",
    place: "in ledger",
    text: "2 links in .claude/skills point into .agents/skills, which is not there.",
    fix: "Remove the links, or put back .agents/skills.",
    skills: ["vendor-api", "zeta"],
    acts: [act("removeLinks", { says: "In ledger, removing them shows as 2 deleted files, and nothing is committed." })],
  };
  const copies: SkillFix = {
    id: "f2",
    kind: "duplicateName",
    title: "capture twice",
    place: "in Cursor",
    text: "Cursor finds both.",
    fix: "Rename one of them.",
    skills: ["capture", "zeta"],
    acts: [act("keep"), act("compare")],
  };
  const command: SkillFix = {
    id: "f3",
    kind: "duplicateName",
    title: "translate twice",
    place: "in Claude, in shop",
    text: "Claude uses the skill over the command.",
    fix: "Remove the old command, or rename one of them.",
    skills: ["translate", "deploy"],
    acts: [act("removeCommand", { refused: "The page leaves the command to you: it is not committed in git, so it could not be brought back." }), act("keep")],
  };
  const only: SkillFix = { id: "f4", kind: "oneCli", title: "flash", place: "for Codex only", text: "", fix: "", skills: ["flash"], acts: [act("keep")] };
  const own: SkillFix = { id: "f5", kind: "nameMismatch", title: "zeta", place: "in ~/.claude/skills", text: "", fix: "Rename it.", skills: ["zeta"], acts: [] };
  const list = (fixes: SkillFix[], kept: SkillFix[] = []): Skills => ({ skills: all, fixes, kept, home: HOME, clis: ["claude", "codex", "cursor"] });
  const overview = (l: Skills, w: Writes | null = writes()) =>
    renderToStaticMarkup(<SkillsOverview list={l} tree={tree(l.skills)} note={null} writes={w} onBack={null} onOpen={() => {}} />);
  const acts = (html: string, title: string) => html.slice(html.indexOf(`<span class="fix-title">${title}</span>`)).split('<div class="fix-acts">')[1]!.split("</div>")[0];

  it("offers what each fix takes, the first as its button, and nothing it cannot do", () => {
    const html = overview(list([links, copies, command, only, own]));
    expect(acts(html, "2 broken links")).toBe(
      '<button type="button" class="btn">Remove 2 links</button><button type="button" class="fix-act" aria-expanded="false">Show them</button>',
    );
    expect(acts(html, "capture twice")).toBe(
      '<button type="button" class="btn">Keep both</button><button type="button" class="fix-act" aria-expanded="false">Compare</button>' +
        '<button type="button" class="fix-act" aria-expanded="false">Show them</button>',
    );
    expect(acts(html, "translate twice")).toMatch(/^<button type="button" class="btn">Keep both<\/button>/);
    expect(html).not.toContain("Remove the command");
    expect(html).toContain(
      '<p class="quiet refused"><span>The page leaves the command to you: it is not committed in git, so it could not be brought back.</span></p>',
    );
    expect(acts(html, "flash")).toMatch(/^<button type="button" class="btn">Keep as is<\/button>/);
    expect(acts(html, "zeta")).toBe('<a class="fix-act" href="/skills/zeta">Open it ›</a>');
    expect(html).toContain("· Keep hides a row until one of its files changes");
    expect(overview(list([links, copies]), null)).not.toMatch(/Remove 2 links|Keep both|Compare/);
  });

  it("asks on its row before a removal, saying what goes and what git shows", () => {
    const html = overview(list([links]), writes({ asking: "fix:f1:removeLinks" }));
    expect(html).toContain(
      '<div class="fix-confirm"><p>Remove these 2 links? Only the links go, each looked at again first: one that changed is left alone.</p>' +
        '<p class="quiet">In ledger, removing them shows as 2 deleted files, and nothing is committed.</p>' +
        '<ul class="fix-skills" aria-label="The links it removes"><li>vendor-api</li><li>zeta</li></ul></div>',
    );
    expect(acts(html, "2 broken links")).toBe(
      '<button type="button" class="btn danger">Remove 2 links</button><button type="button" class="fix-act">Cancel</button>',
    );
    expect(overview(list([links]), writes({ asking: "fix:f9:removeLinks" }))).not.toContain("fix-confirm");
    expect(removeAsk(command, "removeCommand", "~/code/shop/.claude/commands/translate.md")).toBe(
      "Remove ~/code/shop/.claude/commands/translate.md? Only that file goes, and git keeps it.",
    );
    expect(removeAsk({ ...links, skills: ["zeta"] }, "removeLinks", null)).toBe(
      "Remove the link? Only the link goes, looked at again first: if it changed, it is left alone.",
    );
  });

  it("says what an act did and what it left alone, with its Undo, and keeps an error on a row still there", () => {
    const d: SkillDone = {
      said: "Removed 1 link from ledger's .claude/skills.",
      repos: ["ledger: 1 deleted file, nothing committed"],
      skipped: ["`zeta` points somewhere else now, so it was left as it is."],
      undo: "t.1",
    };
    expect(doneText(d)).toBe("Removed 1 link from ledger's .claude/skills. · ledger: 1 deleted file, nothing committed");
    const said = { at: "fix:f1", text: doneText(d), error: false, skipped: d.skipped, undo: d.undo };
    const note = renderToStaticMarkup(<DoneNote said={said} writes={writes()} />);
    expect(note).toBe(
      '<div class="note done" role="status"><div class="done-text"><p>Removed 1 link from ledger&#x27;s .claude/skills. · <span class="together">ledger: 1 deleted file, nothing committed</span></p>' +
        '<p class="skipped"><span><code>zeta</code> points somewhere else now, so it was left as it is.</span></p></div>' +
        '<button type="button" class="btn">Undo</button></div>',
    );
    expect(overview(list([copies]), writes({ said }))).toContain('<div class="note done" role="status">');
    const failed = writes({ said: { at: "fix:f2", text: "It changed.", error: true } });
    const html = overview(list([copies]), failed);
    expect(html).not.toContain("note done");
    expect(html).toContain('<p class="error" role="alert">It changed.</p>');
    expect(overview(list([]), failed), "its row is gone").toContain('<div class="note done error" role="alert">');
  });

  it("puts what is kept under what needs fixing, each with a way back, and counts none of its problems", () => {
    const kept = { ...copies, acts: [act("unkeep"), act("compare")] };
    const skills = all.map((s) => (s.id === "zeta" ? { ...s, problems: [{ kind: "duplicateName" as const, text: "Twice.", kept: true }] } : s));
    const l = { ...list([links], [kept]), skills };
    const html = overview(l);
    expect(html).toContain('<div class="kept"><p class="quiet">1 kept · <button type="button" class="link" aria-expanded="false">show</button></p></div>');
    expect(html).not.toContain("capture twice");
    expect(html, "the vendor link's, not zeta's kept one").toContain("1 problem, one row per fix");
    expect(problemsOf(skills.find((s) => s.id === "zeta")!)).toEqual([]);
    expect(renderToStaticMarkup(<KeptLine list={l} writes={writes()} open />)).toContain(
      '<ul class="kept-list"><li><span class="k-title">capture twice<span class="muted"> in Cursor</span></span><button type="button" class="fix-act">Show again</button></li></ul>',
    );
    const none = overview(list([], [kept]));
    expect(none).toContain('<p class="calm">Nothing needs fixing.</p><div class="kept">');
  });

  it("compares two copies side by side, marking the lines not in the other, or says they are the same", () => {
    const file = (skill: string, path: string, text: string) => ({ skill, file: `${HOME}/${path}/SKILL.md`, text, unread: null });
    const two: SkillCompare = {
      files: [file("a", ".claude/skills/capture", "name: capture\nsame\nmine\n"), file("b", ".codex/skills/capture", "name: capture\nsame\ntheirs\nmore\n")],
      same: false,
    };
    const html = renderToStaticMarkup(<CompareView got={{ ok: true, value: two }} home={HOME} />);
    expect(html).toContain('<div class="compare" role="group" aria-label="The two copies side by side">');
    expect(html).toContain("capture/<wbr/></span><span>SKILL.md</span><span class=\"muted\"> · 1 line not in the other</span></figcaption>");
    expect(html).toContain('<span class="cmp-line">same\n</span><span class="cmp-line differs">mine\n</span>');
    expect(html).toContain("2 lines not in the other");
    const same = renderToStaticMarkup(<CompareView got={{ ok: true, value: { ...two, same: true } }} home={HOME} />);
    expect(same).toMatch(/^<p class="compare same">The two are the same, byte for byte: .*SKILL\.md<\/span> and .*SKILL\.md<\/span>\.<\/p>$/);
    expect(renderToStaticMarkup(<CompareView got={{ ok: false, error: "There is no fix f2." }} home={HOME} />)).toBe(
      '<p class="compare error">There is no fix f2.</p>',
    );
    const row = renderToStaticMarkup(<FixRow fix={copies} list={list([copies])} writes={writes()} onOpen={() => {}} comparing />);
    expect(row).toContain('<button type="button" class="fix-act" aria-expanded="true">Hide the copies</button>');
    expect(row).toContain('<p class="compare muted">Reading the two files…</p>');
  });

  it("marks the lines of a longest run both files share as the same", () => {
    expect(differing(["a", "b", "c"], ["a", "c"])).toEqual([new Set([1]), new Set()]);
    expect(differing(["x"], ["y", "x", "z"])).toEqual([new Set(), new Set([0, 2])]);
    expect(differing([], ["a"])).toEqual([new Set(), new Set([0])]);
    expect(differing(Array(1001).fill("a"), Array(1000).fill("a")), "too long to compare").toBe(null);
  });

  it("names each act, and the other copy by where its CLIs find it", () => {
    expect([actLabel(links, "removeLinks"), actLabel({ ...links, skills: ["a"] }, "removeLinks"), actLabel(command, "removeCommand")]).toEqual([
      "Remove 2 links",
      "Remove the link",
      "Remove the command",
    ]);
    expect([actLabel(copies, "keep"), actLabel({ ...copies, skills: ["a", "b", "c"] }, "keep"), actLabel(only, "keep"), actLabel(copies, "unkeep")]).toEqual([
      "Keep both",
      "Keep all 3",
      "Keep as is",
      "Show again",
    ]);
    const codex = skill("c", { loads: [load({ cli: "codex", path: `${HOME}/.codex/skills/c` }), load({ cli: "cursor", path: `${HOME}/.codex/skills/c` })] });
    expect(copyName(codex, HOME)).toBe("the Codex copy");
    expect(copyName(local("t", "shop"), HOME)).toBe("the shop copy");
    expect(copyName(inRepo("x", `${HOME}/code/toolbox`), HOME)).toBe("the Claude copy");
    expect(copyName(skill("y", { loads: [load({ path: "/opt/skills/y" })] }), HOME)).toBe("the other copy");
  });

  it("offers Keep both and Compare under a skill's own problem, and Show again once kept", () => {
    const codex = skill("capture-codex", {
      name: "capture",
      loads: [load({ cli: "codex", path: `${HOME}/.codex/skills/capture` })],
    });
    const mine = skill("capture", { problems: [{ kind: "duplicateName", text: "Cursor finds another skill named `capture`.", kept: false }] });
    const fix = { ...copies, skills: ["capture", "capture-codex"] };
    const pane = (l: Skills, s = mine) =>
      renderToStaticMarkup(
        <SkillPane skill={s} list={l} tree={tree(l.skills)} detail={null} usage={null} uses={new Map()} writes={writes()} onBack={() => {}} onOpen={() => {}} />,
      );
    const l = { ...list([fix]), skills: [mine, codex] };
    expect(pane(l)).toContain(
      '<div class="fix-acts"><button type="button" class="btn">Keep both</button><button type="button" class="fix-act" aria-expanded="false">Compare with the Codex copy</button></div>',
    );
    const keptMine = { ...mine, problems: [{ ...mine.problems[0]!, kept: true }] };
    const kept = pane({ ...list([], [{ ...fix, acts: [act("unkeep"), act("compare")] }]), skills: [keptMine, codex] }, keptMine);
    expect(kept).toContain('<section class="problems kept" aria-label="1 problem">');
    expect(kept).toContain("<span class=\"muted\"> Kept off what needs fixing until one of its files changes.</span>");
    expect(kept).toContain('<button type="button" class="btn">Show again</button>');
  });
});
