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
    expect(phone).toMatch(/\n {2}input,\n {2}textarea,[^{]*\{\n {4}font-size: 16px;/);
  });
});
