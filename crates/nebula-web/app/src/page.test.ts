import { readdirSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const here = new URL(".", import.meta.url);
const source = (name: string) => readFileSync(new URL(name, here), "utf8");

describe("the page everywhere", () => {
  it("keeps a key a button shows out of the button's name", () => {
    for (const name of readdirSync(here).filter((f) => f.endsWith(".tsx") && !f.includes(".test."))) {
      expect(source(name).match(/<kbd(?![^>]*aria-hidden="true")/g), name).toBeNull();
    }
  });

  it("sets every field in 16px on a phone, so the phone does not zoom in on one it focuses", () => {
    const css = source("app.css");
    const phone = css.slice(css.indexOf("@media (max-width: 767px)"));
    expect(phone).toMatch(/\n {2}input,\n {2}select,\n {2}textarea,[^{]*\{\n {4}font-size: 16px;/);
  });

  it("starts a Home Screen app below the status bar, as iOS 27 blurs what runs under a black-translucent one", () => {
    const html = readFileSync(new URL("../index.html", here), "utf8");
    expect(html).toContain('<meta name="apple-mobile-web-app-status-bar-style" content="default" />');
    expect(html).not.toContain("black-translucent");
  });

  it("keeps every head's content below that blur in an app added under a black-translucent status bar", () => {
    for (const name of ["app.css", "palette.css"]) {
      expect(source(name).match(/padding[^;]*safe-area-inset-top/g), name).toBeNull();
    }
    expect(source("app.css")).toContain(
      "--top-inset: calc(env(safe-area-inset-top, 0px) + min(env(safe-area-inset-top, 0px), 40px));",
    );
  });
});
