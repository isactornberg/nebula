// The paired devices and the pairing open for the next, as `nebula serve`
// keeps them for the Mac's page. A pairing the page opens is taken by an
// "iPhone" after PAIR_MS, so the panel is seen to say so.
// `NEBULA_MOCK_TAILNET=none pnpm dev` has no Tailscale running.
import type { Device, Devices } from "../api/contract.ts";

const TAILNET = "my-mac.tailnet-name.ts.net";
const CODE = "K7QXM2PD";
/// How long a pairing pairs, as the server's.
const PAIRING_MS = 5 * 60_000;
/// How soon the stand-in phone takes the code.
const PAIR_MS = 12_000;

/// The QR code of `https://my-mac.tailnet-name.ts.net/pair/K7QXM2PD`, as
/// the server draws it.
const QR = [
  "111111100110111011011001001111111",
  "100000100000011101001110001000001",
  "101110101011010111101110101011101",
  "101110101010101110001000101011101",
  "101110101010000100100110001011101",
  "100000101111110110001000101000001",
  "111111101010101010101010101111111",
  "000000001101001100100110100000000",
  "101111100100010111100001101111100",
  "110100000101000010111101001101111",
  "001011111111000100100000000010110",
  "001001001000001010011100001011110",
  "101011100110100000110111000011010",
  "010011011000101111001010001001111",
  "010011100000110000010010011000110",
  "010110011100101010011100111110100",
  "110110110010111001010010110110001",
  "101100010111010001111111001101101",
  "110001100011111111000100011110110",
  "000100001011110111101100011111101",
  "111011110111000110011111010111011",
  "111011001100001101100100000001001",
  "101000111000101111001110011001010",
  "101111011100110100100101010101100",
  "100101111001001101000010111111000",
  "000000001100110010111100100010101",
  "111111100100111110101111101010110",
  "100000101100000110000100100011101",
  "101110101011000000110100111111011",
  "101110101011010110001101110010111",
  "101110101000110000110100011100100",
  "100000100111100010011111001011100",
  "111111101010011001010010111100010",
];

export class MockDevices {
  private devices: Device[];
  private pairing: { until: number; timer: ReturnType<typeof setTimeout> } | null = null;

  constructor(
    private readonly tailnet: boolean,
    now: number,
  ) {
    this.devices = [
      {
        id: "3f9a1c0e5b7d2a64",
        name: "iPad",
        login: "ada@example.com",
        pairedAt: now - 9 * 86_400_000,
        lastUsed: now - 2 * 3_600_000,
      },
    ];
  }

  list(now: number): Devices {
    const open = this.pairing && this.pairing.until > now ? this.pairing : null;
    return {
      tailnet: this.tailnet ? TAILNET : null,
      devices: [...this.devices].sort((a, b) => b.pairedAt - a.pairedAt),
      pairing: open
        ? { code: `${CODE.slice(0, 4)}-${CODE.slice(4)}`, url: `https://${TAILNET}/pair/${CODE}`, qr: QR, expiresAt: open.until }
        : null,
    };
  }

  pair(now: number): { status: number; body: unknown } {
    if (!this.tailnet)
      return {
        status: 409,
        body: { error: "Tailscale is not running on this Mac, so no phone can reach this page: `tailscale status` names no tailnet for it." },
      };
    if (this.pairing) clearTimeout(this.pairing.timer);
    const timer = setTimeout(() => {
      this.pairing = null;
      const at = Date.now();
      this.devices.push({ id: at.toString(16), name: "iPhone", login: "ada@example.com", pairedAt: at, lastUsed: at });
    }, PAIR_MS);
    this.pairing = { until: now + PAIRING_MS, timer };
    return { status: 200, body: this.list(now) };
  }

  remove(id: string, now: number): { status: number; body: unknown } {
    const before = this.devices.length;
    this.devices = this.devices.filter((d) => d.id !== id);
    if (this.devices.length === before) return { status: 404, body: { error: "There is no such paired device." } };
    return { status: 200, body: this.list(now) };
  }
}
