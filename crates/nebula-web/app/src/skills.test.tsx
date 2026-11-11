import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Skill, SkillLoad } from "./api/contract";
import { parseRoute, routePath, sectionOf } from "./model";
import { bestMatch, filterSkills, groups, skillKey, skillToOpen, tilde, valueText, whereOf } from "./skills";
import { SkillsSide } from "./SkillsPage";

const HOME = "/home/me";

const load = (over: Partial<SkillLoad> = {}): SkillLoad => ({
  cli: "claude",
  name: "x",
  scope: "personal",
  path: `${HOME}/.claude/skills/x`,
  project: null,
  space: null,
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

const inRepo = (id: string, root: string) =>
  skill(id, { path: `${root}/skills/${id}`, source: { kind: "repo" }, repo: { root, branch: "main" } });

const all = [
  skill("zeta"),
  inRepo("capture", `${HOME}/code/toolbox`),
  skill("lint", {
    source: { kind: "plugin", plugin: "kit", version: "1.0.0", marketplace: "market", from: null },
    loads: [load({ name: "kit:lint", scope: "plugin" })],
  }),
  inRepo("alpha", `${HOME}/code/toolbox`),
  skill("imagegen", { source: { kind: "bundled", cli: "codex" }, loads: [load({ cli: "codex", scope: "system" })] }),
  skill("docs", { source: { kind: "claudeAi" } }),
  skill("watch", { source: { kind: "installer", source: "acme/video", url: null, path: null } }),
];

const key = (k: string) => ({ key: k, metaKey: false, ctrlKey: false, altKey: false });

describe("skills routes", () => {
  it("parses the list and a skill, and round-trips an id", () => {
    expect(parseRoute("/skills")).toEqual({ kind: "skills" });
    expect(parseRoute("/skills/")).toEqual({ kind: "skills" });
    expect(parseRoute("/skills/abc123")).toEqual({ kind: "skill", id: "abc123" });
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
  });
});

describe("the skills list", () => {
  it("groups by where a skill comes from, the user's own first, each by name", () => {
    const got = groups(all, HOME).map((g) => [g.title, g.skills.map((s) => s.name)]);
    expect(got).toEqual([
      ["toolbox", ["alpha", "capture"]],
      ["~/.claude/skills", ["zeta"]],
      ["npx skills", ["watch"]],
      ["kit", ["lint"]],
      ["claude.ai", ["docs"]],
      ["Codex", ["imagegen"]],
    ]);
    expect(groups(all, HOME)[0]!.note).toBe("~/code/toolbox · main");
  });

  it("filters by every word, on names, a CLI's name for it, and the group", () => {
    const names = (q: string) => filterSkills(all, q, HOME).map((s) => s.name);
    expect(names("")).toHaveLength(all.length);
    expect(names("CAP")).toEqual(["capture"]);
    expect(names("kit:")).toEqual(["lint"]);
    expect(names("toolbox a")).toEqual(["capture", "alpha"]);
    expect(names("nothing here")).toEqual([]);
  });

  it("draws the groups with links to each skill, the open one current", () => {
    const html = renderToStaticMarkup(
      <SkillsSide
        got={{ ok: true, value: { skills: all, home: HOME, clis: ["claude", "codex"] } }}
        groups={groups(all, HOME)}
        clis={["claude", "codex"]}
        cursor="alpha"
        openId="capture"
        query=""
        filter={{ current: null }}
        onQuery={() => {}}
        onFilterKey={() => {}}
        onOpen={() => {}}
      />,
    );
    expect(html).toMatch(/>toolbox <span class="group-count">2<\/span><\/h2>/);
    expect(html).toContain('<a class="row skill-row" data-cursor="true" href="/skills/alpha">');
    expect(html).toContain('<a class="row skill-row" aria-current="page" href="/skills/capture">');
    expect(html).toContain('aria-label="Loaded by Claude"');
    expect(html).toContain('placeholder="Filter skills"');
  });

  it("says so when a filter leaves nothing", () => {
    const html = renderToStaticMarkup(
      <SkillsSide
        got={{ ok: true, value: { skills: all, home: HOME, clis: ["claude"] } }}
        groups={[]}
        clis={["claude"]}
        cursor={null}
        openId={null}
        query="zzz"
        filter={{ current: null }}
        onQuery={() => {}}
        onFilterKey={() => {}}
        onOpen={() => {}}
      />,
    );
    expect(html).toContain("No skill matches the filter.");
  });
});

describe("skills keys", () => {
  it("moves, opens and goes back as the Attention list does, and / goes to the filter", () => {
    expect(skillKey(key("j"), false, "")).toEqual({ kind: "move", delta: 1 });
    expect(skillKey(key("ArrowUp"), false, "")).toEqual({ kind: "move", delta: -1 });
    expect(skillKey(key("Enter"), false, "")).toEqual({ kind: "open" });
    expect(skillKey(key("Escape"), false, "")).toEqual({ kind: "back" });
    expect(skillKey(key("/"), false, "")).toEqual({ kind: "filter" });
    expect(skillKey(key("t"), false, "")).toBe(null);
    expect(skillKey(key("1"), false, "")).toBe(null);
    expect(skillKey({ ...key("/"), metaKey: true }, false, "")).toBe(null);
  });

  it("types letters in the filter, which still moves and opens, and clears it before leaving", () => {
    expect(skillKey(key("j"), true, "")).toBe(null);
    expect(skillKey(key("ArrowDown"), true, "")).toEqual({ kind: "move", delta: 1 });
    expect(skillKey(key("Enter"), true, "x")).toEqual({ kind: "open" });
    expect(skillKey(key("Escape"), true, "x")).toEqual({ kind: "clear" });
    expect(skillKey(key("Escape"), true, "")).toEqual({ kind: "leave" });
  });

  it("opens the skill under the cursor, else the one a filter means most", () => {
    const items = groups(all, HOME).flatMap((g) => g.skills);
    expect(skillToOpen(items, "watch", "")?.name).toBe("watch");
    expect(skillToOpen(items, null, "")).toBe(undefined);
    expect(skillToOpen(items, null, "a")?.name).toBe("alpha");
    expect(skillToOpen(items, null, "ture")?.name).toBe("capture");
    const kit = [skill("format", { source: all[2]!.source }), skill("lint", { source: all[2]!.source })];
    expect(bestMatch(filterSkills(kit, "kit", HOME), "kit")?.name).toBe("format");
    expect(bestMatch(kit, "lint")?.name).toBe("lint");
  });
});

describe("skills text", () => {
  it("shows paths under the home as ~", () => {
    expect(tilde(`${HOME}/code`, HOME)).toBe("~/code");
    expect(tilde(HOME, HOME)).toBe("~");
    expect(tilde("/home/meadow/x", HOME)).toBe("/home/meadow/x");
  });

  it("says where a load is", () => {
    expect(whereOf(load())).toBe("personal");
    expect(whereOf(load({ scope: "project", project: "shop" }))).toBe("project shop");
    expect(whereOf(load({ scope: "nested", project: "shop" }))).toBe("nested in shop");
    expect(whereOf(load({ scope: "addedDir", space: "Acme" }))).toBe("space Acme");
    expect(whereOf(load({ scope: "system" }))).toBe("bundled");
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
