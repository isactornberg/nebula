import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Device, Devices } from "./api/contract";
import { when } from "./model";
import { justPaired, onThisMachine, PhonesView, qrPath } from "./Phones";

const NOW = 100 * 86_400_000;
const TAILNET = "my-mac.tailnet-name.ts.net";

const device = (id: string, over: Partial<Device> = {}): Device => ({
  id,
  name: "iPhone",
  login: "ada@example.com",
  pairedAt: NOW - 3_600_000,
  lastUsed: NOW - 120_000,
  push: false,
  ...over,
});

const devices = (over: Partial<Devices> = {}): Devices => ({
  tailnet: TAILNET,
  devices: [],
  pairing: null,
  ...over,
});

const pairing = { code: "K7QX-M2PD", url: `https://${TAILNET}/pair/K7QXM2PD`, qr: ["101", "010", "101"], expiresAt: NOW + 245_000 };

const view = (d: Devices, over: Partial<Parameters<typeof PhonesView>[0]> = {}) =>
  renderToStaticMarkup(
    <PhonesView
      got={{ ok: true, value: d }}
      now={NOW}
      busy={null}
      error={null}
      note={null}
      port="7682"
      onPair={() => {}}
      onRemove={() => {}}
      onClose={() => {}}
      {...over}
    />,
  );

describe("Phones", () => {
  it("is offered on this machine alone, not over the tailnet", () => {
    expect(onThisMachine("127.0.0.1")).toBe(true);
    expect(onThisMachine("localhost")).toBe(true);
    expect(onThisMachine(TAILNET)).toBe(false);
  });

  it("draws each dark module of the QR code inside a quiet zone of four", () => {
    expect(qrPath(["10", "01"])).toBe("M4 4h1v1h-1zM5 5h1v1h-1z");
  });

  it("says which device took the code once the pairing it showed is gone", () => {
    const before = devices({ devices: [device("a")], pairing });
    expect(justPaired(before, devices({ devices: [device("b"), device("a")] }))?.id).toBe("b");
    expect(justPaired(before, devices({ devices: [device("a")] }))).toBeNull();
    expect(justPaired(devices({ devices: [device("a")] }), devices({ devices: [device("b")] }))).toBeNull();
    expect(justPaired(before, devices({ devices: [device("b")], pairing }))).toBeNull();
  });

  it("shows an open pairing's QR code, link and code, and how long it pairs", () => {
    const html = view(devices({ pairing }));
    expect(html).toContain(`aria-label="QR code of https://${TAILNET}/pair/K7QXM2PD"`);
    expect(html).toContain('viewBox="0 0 11 11"');
    expect(html).toContain(`https://${TAILNET}/<wbr/>pair/K7QXM2PD`);
    expect(html).toContain('<strong class="pairing-code">K7QX-M2PD</strong>');
    expect(html).toContain("for 4:05 more.");
    expect(html).not.toContain("Pair a phone</button>");
    expect(html).toContain("<code>tailscale serve --bg 7682</code>");
  });

  it("offers a new code once the last one expired", () => {
    const html = view(devices({ pairing: { ...pairing, expiresAt: NOW - 1 } }));
    expect(html).toContain("Pair a phone</button>");
    expect(html).toContain("The code expired: make a new one to pair a phone.");
    expect(html).not.toContain("qr-dark");
  });

  it("lists the paired devices with who paired each and a Remove", () => {
    const html = view(devices({ devices: [device("a"), device("b", { name: "iPad" })] }));
    expect(html).toContain(
      `<span class="quiet">ada@example.com · paired ${when(NOW - 3_600_000, NOW)} · used 2 min ago</span>`,
    );
    expect(html).toContain('<span class="device-name">iPhone<span class="device-push">');
    expect(html.match(/>Remove<\/button>/g)).toHaveLength(2);
    expect(view(devices())).toContain("No device is paired yet.");
  });

  it("says which devices have notifications on, and offers each of those a test", () => {
    const d = devices({ devices: [device("a", { push: true }), device("b", { name: "iPad" })] });
    const html = view(d);
    expect(html).toContain('<span class="device-push on"><span aria-hidden="true" class="dot tone-green"></span>Notifications on</span>');
    expect(html).toContain('<span class="device-push"><span aria-hidden="true" class="dot tone-hollow"></span>Notifications off</span>');
    expect(html.match(/>Send a test<\/button>/g)).toHaveLength(1);
    const sent = view(d, { tested: { id: "a", text: "Sent a test to iPhone.", ok: true } });
    expect(sent).toContain('<span class="ok" role="status">Sent a test to iPhone.</span>');
    const busy = view(d, { busy: "test:a" });
    expect(busy).toContain("disabled=\"\"");
    const refused = view(d, { tested: { id: "a", text: "The test did not reach iPhone.", ok: false } });
    expect(refused).toContain('<span class="error" role="status">The test did not reach iPhone.</span>');
  });

  it("says no phone can reach the page while Tailscale is not running", () => {
    const html = view(devices({ tailnet: null }));
    expect(html).toContain("Tailscale is not running on this Mac, so no phone can reach this page.");
    expect(html).not.toContain("Pair a phone</button>");
    expect(html).not.toContain("tailscale serve");
  });
});
