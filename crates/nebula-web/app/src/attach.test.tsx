import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ImageStrip, imagesHold, pastedImages, sizeOf, type Attached } from "./Attach";
import { unsent } from "./Compose";

const image = (key: number, over: Partial<Attached> = {}): Attached => ({
  key,
  name: `shot-${key}.png`,
  size: 384 * 1024,
  url: `blob:http://127.0.0.1:7682/${key}`,
  sent: 0,
  id: null,
  error: null,
  ...over,
});

describe("a follow-up once it went", () => {
  it("leaves the box what was typed in it while it was sending", () => {
    expect(unsent("Merge it", "Merge it")).toBe("");
    expect(unsent("  Merge it\n", "Merge it")).toBe("");
    expect(unsent("Merge it and then rebase", "Merge it")).toBe("and then rebase");
    expect(unsent("Merge it\nAlso the docs", "Merge it")).toBe("Also the docs");
  });

  it("keeps what replaced the text sent, and what was typed to images alone", () => {
    expect(unsent("Wait, not yet", "Merge it")).toBe("Wait, not yet");
    expect(unsent("", "Merge it")).toBe("");
    expect(unsent("A caption", "")).toBe("A caption");
  });
});

describe("a follow-up's images", () => {
  it("takes the images of a paste and leaves a copied document", () => {
    const png = new File(["x"], "image.png", { type: "image/png" });
    const heic = new File(["x"], "IMG_0001.HEIC", { type: "image/heic" });
    const pdf = new File(["x"], "notes.pdf", { type: "application/pdf" });
    expect(pastedImages([png, pdf, heic])).toEqual([png, heic]);
    expect(pastedImages([])).toEqual([]);
  });

  it("holds the send while one uploads or failed, and lets it go once all are kept", () => {
    expect(imagesHold([])).toBeNull();
    expect(imagesHold([image(1, { id: "web-a.png" }), image(2, { sent: 0.4 })])).toBe("Uploading the image…");
    expect(imagesHold([image(1, { id: "web-a.png" }), image(2, { error: "No." })])).toBe("Remove the image that did not upload first.");
    expect(imagesHold([image(1, { id: "web-a.png" }), image(2, { id: "web-b.jpg" })])).toBeNull();
  });

  it("shows each one's picture, its upload, why it failed, and a way to remove it", () => {
    const html = renderToStaticMarkup(
      <ImageStrip
        images={[image(1, { id: "web-a.png", sent: 1 }), image(2, { sent: 0.5 }), image(3, { error: "That is not an image nebula can send." })]}
        onRemove={() => {}}
      />,
    );
    expect(html).toContain('<img src="blob:http://127.0.0.1:7682/1" alt="shot-1.png"/>');
    expect(html.match(/role="progressbar"/g)).toHaveLength(1);
    expect(html).toContain('aria-label="Uploading shot-2.png" aria-valuemin="0" aria-valuemax="100" aria-valuenow="50"');
    expect(html).toContain('class="thumb failed"');
    expect(html).toContain('<p class="error" role="alert">That is not an image nebula can send.</p>');
    for (const n of [1, 2, 3]) expect(html).toContain(`aria-label="Remove shot-${n}.png"`);
    expect(renderToStaticMarkup(<ImageStrip images={[]} onRemove={() => {}} />)).toBe("");
  });

  it("names each one with its size and where its upload stands, where the composer has room", () => {
    const html = renderToStaticMarkup(
      <ImageStrip
        named
        images={[image(1, { id: "web-a.png", sent: 1 }), image(2, { sent: 0.5 }), image(3, { error: "No." })]}
        onRemove={() => {}}
      />,
    );
    expect(html).toContain('<span class="thumb-label">shot-1.png</span><span class="thumb-state">384 KB · uploaded</span>');
    expect(html).toContain('<span class="thumb-state">384 KB · 50%</span>');
    expect(html).toContain('<span class="thumb-state">384 KB · not sent</span>');
    expect(renderToStaticMarkup(<ImageStrip images={[image(1)]} onRemove={() => {}} />)).not.toContain("thumb-label");
  });

  it("says a size in KB, and past a megabyte in MB", () => {
    expect(sizeOf(300)).toBe("1 KB");
    expect(sizeOf(384 * 1024)).toBe("384 KB");
    expect(sizeOf(4.2 * 1024 * 1024)).toBe("4.2 MB");
  });
});
