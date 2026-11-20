import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { NewSession, Project } from "./api/contract";
import { parseRoute, routePath } from "./model";
import { afterStart, firstProject, keepProject, newBody, NewSessionPage, StartedAway, startedNote, startNew } from "./NewSession";

const PROJECTS: Project[] = [
  { id: "p-shop", name: "shopfront", space: null, team: "ACME" },
  { id: "p-api", name: "api", space: null, team: "ACME" },
  { id: "p-docs", name: "docs-site", space: null, team: null },
];

/// A device's storage, holding `kept` as its last project.
const storage = (kept: string | null = null) => {
  const items = new Map<string, string>(kept ? [["nebula.newProject", kept]] : []);
  return {
    getItem: vi.fn((k: string) => items.get(k) ?? null),
    setItem: vi.fn((k: string, v: string) => void items.set(k, v)),
  };
};

const broken = {
  getItem: () => {
    throw new Error("blocked");
  },
  setItem: () => {
    throw new Error("blocked");
  },
};

describe("New's project", () => {
  it("is the one the list is filtered to", () => {
    expect(firstProject(PROJECTS, "p-docs", storage("p-api"))).toBe("p-docs");
  });

  it("is else the one this device started a session in last", () => {
    expect(firstProject(PROJECTS, null, storage("p-api"))).toBe("p-api");
  });

  it("is else the first, as it is for a project gone or a device that kept none", () => {
    expect(firstProject(PROJECTS, null, storage())).toBe("p-shop");
    expect(firstProject(PROJECTS, "p-gone", storage("p-old"))).toBe("p-shop");
    expect(firstProject(PROJECTS, null, undefined)).toBe("p-shop");
    expect(firstProject(PROJECTS, null, broken)).toBe("p-shop");
    expect(firstProject([], null, storage("p-api"))).toBeNull();
  });

  it("is remembered on this device, and a storage that refuses is no failure", () => {
    const kept = storage();
    keepProject(kept, "p-docs");
    expect(firstProject(PROJECTS, null, kept)).toBe("p-docs");
    expect(() => keepProject(broken, "p-docs")).not.toThrow();
  });
});

describe("New's request", () => {
  it("names the project and the prompt, and its images in the order added", () => {
    expect(newBody("p-api", "Rotate the key", [])).toEqual({ projectId: "p-api", text: "Rotate the key" });
    expect(newBody("p-api", "", ["web-b.png", "web-a.png"])).toEqual({
      projectId: "p-api",
      text: "",
      images: ["web-b.png", "web-a.png"],
    });
  });

  it("asks the server for the session, and remembers its project once it stands", async () => {
    const kept = storage();
    const post = vi.fn(async (_: NewSession) => ({ ok: true as const, value: { id: "agent-7", problem: null } }));
    const got = await startNew("p-docs", "Tidy the README", ["web-a.png"], kept, post);
    expect(post).toHaveBeenCalledWith({ projectId: "p-docs", text: "Tidy the README", images: ["web-a.png"] });
    expect(got).toEqual({ ok: true, value: { id: "agent-7", problem: null } });
    expect(kept.setItem).toHaveBeenCalledWith("nebula.newProject", "p-docs");
  });

  it("opens the session the server started in New's place, saying why its prompt may not be whole", () => {
    expect(afterStart("/new", { id: "agent-7", problem: null })).toEqual({
      kind: "open",
      to: { kind: "session", id: "agent-7" },
      note: null,
    });
    const problem = "The session asks whether to trust its folder, so nothing was typed.";
    expect(afterStart("/new/", { id: "agent-7", problem })).toEqual({
      kind: "open",
      to: { kind: "session", id: "agent-7" },
      note: `The session started. ${problem}`,
    });
  });

  it("leaves a page gone to since Start alone, saying there that the session started", () => {
    expect(afterStart("/s/agent-3", { id: "agent-7", problem: null })).toEqual({ kind: "away" });
    expect(afterStart("/", { id: "agent-7", problem: null })).toEqual({ kind: "away" });
    const html = renderToStaticMarkup(<StartedAway got={{ id: "agent-7", problem: null }} onOpen={() => {}} />);
    expect(html).toBe('Your new session started. <a href="/s/agent-7">Open it</a>');
  });

  it("says why a session that stands may not have its prompt whole, and that it started", () => {
    expect(startedNote({ id: "agent-7", problem: null })).toBeNull();
    expect(startedNote({ id: "agent-7", problem: "The session asks whether to trust its folder, so nothing was typed." })).toBe(
      "The session started. The session asks whether to trust its folder, so nothing was typed.",
    );
  });

  it("passes a failure on as it is, remembering nothing", async () => {
    const kept = storage();
    const post = async () => ({ ok: false as const, error: "There is no project p-gone." });
    expect(await startNew("p-gone", "Hello", [], kept, post)).toEqual({ ok: false, error: "There is no project p-gone." });
    expect(kept.setItem).not.toHaveBeenCalled();
  });
});

describe("New's page", () => {
  const html = (filter: string | null, layout: "wide" | "phone" = "wide") =>
    renderToStaticMarkup(
      <NewSessionPage projects={PROJECTS} filter={filter} layout={layout} back="Inbox" onBack={() => {}} onStarted={() => {}} />,
    );

  it("is at /new", () => {
    expect(parseRoute("/new")).toEqual({ kind: "new" });
    expect(parseRoute("/new/")).toEqual({ kind: "new" });
    expect(routePath({ kind: "new" })).toBe("/new");
  });

  it("picks the filtered project, offers every one, and starts from the thread's own composer", () => {
    const out = html("p-api");
    expect(out).toMatch(/<option value="p-api" selected="">api<\/option>/);
    expect(out.match(/<option /g)).toHaveLength(3);
    expect(out).toContain('id="new-text"');
    expect(out).toContain("What should Claude do in api?");
    expect(out).toMatch(/<button type="submit" class="send" disabled="">Start<kbd/);
    expect(out).toContain("New session in api");
  });

  it("on a phone has Back to the list and the round buttons", () => {
    const out = html(null, "phone");
    expect(out).toContain('class="sess-back"');
    expect(out).toContain(">Inbox</a>");
    expect(out).toContain('aria-label="Dictate the prompt"');
    expect(out).toContain('placeholder="What should Claude do?"');
  });

  it("says so where there is no project to start in", () => {
    const out = renderToStaticMarkup(
      <NewSessionPage projects={[]} filter={null} layout="wide" back="Inbox" onBack={() => {}} onStarted={() => {}} />,
    );
    expect(out).toContain("no project to start a session in yet");
    expect(out).toContain("No project to start a session in.");
  });
});
