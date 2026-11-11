import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Project, Skill, SkillFix, SkillLoad, Skills, SkillUsage } from "./api/contract";
import { parseRoute, routePath, sectionOf } from "./model";
import {
  bestItem,
  filterTree,
  groupUsed,
  placeOf,
  rowUsed,
  sessionGets,
  skillKey,
  skillsSummary,
  skillTree,
  tilde,
  usedLabel,
  usedText,
  usesOf,
  valueText,
  visibleItems,
  whereItLoads,
  type Section,
} from "./skills";
import { SkillsOverview, SkillsSide } from "./SkillsPage";

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
    problems: [{ kind: "brokenLink", text: "The link points at a folder that is not there." }],
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
  const got = { ok: true as const, value: { skills: all, fixes: [], home: HOME, clis: ["claude", "codex", "cursor"] } as Skills };
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
    const fix: SkillFix = { kind: "oneCli", title: "zeta", place: "for Claude only", text: "", fix: "", skills: ["zeta"] };
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
        kind: "brokenLink",
        title: "2 broken links",
        place: "in ledger",
        text: "2 links in .claude/skills point into .agents/skills, which is not there.",
        fix: "Remove the links, or put back .agents/skills.",
        skills: ["vendor-api", "zeta"],
      },
      { kind: "nameMismatch", title: "zeta", place: "in ~/.claude/skills", text: "The name is `Zeta`.", fix: "Rename it.", skills: ["zeta"] },
    ];
    const list = { ...got.value, fixes, skills: all.map((s) => (s.id === "zeta" ? { ...s, problems: [fixes[1]!] } : s)) };
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
  const list: Skills = { skills: all, fixes: [], home: HOME, clis: ["claude", "codex", "cursor"] };

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
