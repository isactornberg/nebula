import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ImageStrip, imagesHold, pastedImages, type Attached } from "./Attach";
import { Composer } from "./Detail";
import { session } from "./mock/fixtures";

const image = (key: number, over: Partial<Attached> = {}): Attached => ({
  key,
  name: `shot-${key}.png`,
  url: `blob:http://127.0.0.1:7682/${key}`,
  sent: 0,
  id: null,
  error: null,
  ...over,
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

  it("offers a phone's photos and camera beside Send", () => {
    const s = session(0, { id: "rover-1", name: "Rotate the key", projectId: "p-rover", status: "finished", ago: 2 });
    const html = renderToStaticMarkup(<Composer session={s} dialogOpen={false} answered={false} busy={false} onSend={async () => true} />);
    expect(html).toContain('<input type="file" accept="image/*" multiple="" hidden=""/>');
    expect(html).toMatch(/<button type="button" class="btn attach">.*Image<\/button>/);
  });
});
