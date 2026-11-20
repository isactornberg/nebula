import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { choose, chosen, themeOf } from "./theme";

/// A device's storage, as a map.
const storage = () => {
  const kept = new Map<string, string>();
  return { getItem: (k: string) => kept.get(k) ?? null, setItem: (k: string, v: string) => void kept.set(k, v) };
};

/// Storage a private window or blocked site data gives: every call throws.
const blocked = {
  getItem: (): string | null => {
    throw new Error("SecurityError");
  },
  setItem: () => {
    throw new Error("QuotaExceededError");
  },
};

describe("the theme", () => {
  it("follows the system's until this device chooses one", () => {
    expect(themeOf(storage(), true)).toBe("light");
    expect(themeOf(storage(), false)).toBe("dark");
  });

  it("keeps the device's choice over the system's, read again later", () => {
    const s = storage();
    choose(s, "light");
    expect(chosen(s)).toBe("light");
    expect(themeOf(s, false)).toBe("light");
    choose(s, "dark");
    expect(themeOf(s, true)).toBe("dark");
  });

  it("falls back to the system's where storage throws or holds something else", () => {
    expect(() => choose(blocked, "light")).not.toThrow();
    expect(chosen(blocked)).toBe(null);
    expect(themeOf(blocked, true)).toBe("light");
    expect(themeOf(null, false)).toBe("dark");
    const odd = storage();
    odd.setItem("nebula.theme", "solarized");
    expect(themeOf(odd, false)).toBe("dark");
  });

  it("is read before the first paint by the key it is kept under", () => {
    const s = storage();
    choose(s, "light");
    expect(firstPaint("/", s, false).theme).toBe("light");
    choose(s, "dark");
    expect(firstPaint("/", s, true).theme).toBe("dark");
    expect(firstPaint("/", storage(), true).theme).toBe("light");
    expect(firstPaint("/", blocked, false).theme).toBe("dark");
  });

  it("puts the browser's bar in the page's top colour before the first paint", () => {
    const tokens = readFileSync(new URL("./tokens.css", import.meta.url), "utf8");
    const token = (block: string, name: string) => new RegExp(`${block} \\{[^}]*--${name}: (#\\w+);`).exec(tokens)![1];
    const s = storage();
    expect(firstPaint("/", s, false).bar).toBe(token(":root", "bg"));
    expect(firstPaint("/deck", s, false).bar).toBe(token(":root", "sunk"));
    choose(s, "light");
    expect(firstPaint("/s/a", s, false).bar).toBe(token(':root\\[data-theme="light"\\]', "bg"));
    expect(firstPaint("/deck", s, false)).toEqual({
      theme: "light",
      bar: token(':root\\[data-theme="light"\\]', "sunk"),
      manifest: "/deck.webmanifest",
    });
  });
});

/// What index.html's first script leaves on the page at `path`, run as the
/// browser runs it before anything is drawn, with `kept` its storage and
/// the system's theme light or not.
function firstPaint(path: string, kept: Pick<Storage, "getItem">, systemLight: boolean) {
  const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
  const script = /<script>([^]*?)<\/script>/.exec(html)![1]!;
  const root = { dataset: {} as Record<string, string> };
  const bar = { content: "" };
  const manifest = { href: "/manifest.webmanifest" };
  const document = {
    documentElement: root,
    querySelector: (sel: string) => (sel === 'meta[name="theme-color"]' ? bar : sel === 'link[rel="manifest"]' ? manifest : null),
  };
  const matchMedia = (q: string) => ({ matches: q === "(prefers-color-scheme: light)" && systemLight });
  new Function("document", "localStorage", "matchMedia", "location", script)(document, kept, matchMedia, { pathname: path });
  return { theme: root.dataset.theme, bar: bar.content, manifest: manifest.href };
}
