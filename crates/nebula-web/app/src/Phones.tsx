import { useEffect, useRef, useState } from "react";
import { api, type Read } from "./api/client";
import type { Device, Devices } from "./api/contract";
import { ago, when } from "./model";
import { Icon } from "./ui";

/// How often the open panel reads the devices again, to see a phone pair.
const POLL_MS = 2000;

/// Whether the page is open on this machine itself rather than over the
/// tailnet: only there does it pair a device or remove one.
export function onThisMachine(host = location.hostname): boolean {
  return host === "127.0.0.1" || host === "localhost";
}

/// The dark modules of a QR code as one SVG path, inside a quiet zone of
/// four modules.
export function qrPath(rows: string[]): string {
  let d = "";
  rows.forEach((row, y) => {
    for (let x = 0; x < row.length; x++) if (row[x] === "1") d += `M${x + 4} ${y + 4}h1v1h-1z`;
  });
  return d;
}

/// The device a read of the devices shows paired since the one before,
/// while that one had a pairing open.
export function justPaired(before: Devices | null, now: Devices): Device | null {
  if (!before?.pairing || now.pairing) return null;
  return now.devices.find((d) => !before.devices.some((b) => b.id === d.id)) ?? null;
}

/// What the panel says of the pairing it showed last: green for a pair.
export type Note = { text: string; ok: boolean };

/// What came of the test last sent to the device `id`.
export type Tested = Note & { id: string };

const EXPIRED: Note = { text: "The code expired: make a new one to pair a phone.", ok: false };

/// `ms` as minutes and seconds, `4:05`.
function countdown(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/// Phones in the rail or the top bar, which opens the panel.
export function PhonesButton({ onOpen }: { onOpen: () => void }) {
  return (
    <button
      type="button"
      className="phones-open"
      aria-haspopup="dialog"
      title="Pair a phone to reach this page from it"
      onClick={onOpen}
    >
      <svg
        aria-hidden="true"
        width="14"
        height="14"
        viewBox="0 0 24 24"
        fill="none"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <rect x="6" y="2" width="12" height="20" rx="2.5" />
        <line x1="11" y1="18" x2="13" y2="18" />
      </svg>
      <span className="item-name">Phones</span>
    </button>
  );
}

/// The Phones panel, while `open`: pair a phone, and the devices paired,
/// each with whether its notifications are on, Send a test while they
/// are, and Remove.
export function PhonesDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = dialog.current!;
    if (open && !d.open) d.showModal();
    if (!open && d.open) d.close();
  }, [open]);
  return (
    <dialog
      ref={dialog}
      className="phones"
      aria-labelledby="phones-title"
      onClose={onClose}
      // The page's keys stay out of the panel: Esc closes it alone.
      onKeyDown={(e) => e.stopPropagation()}
      onClick={(e) => e.target === dialog.current && dialog.current.close()}
    >
      {open && <PhonesPanel onClose={() => dialog.current?.close()} />}
    </dialog>
  );
}

function PhonesPanel({ onClose }: { onClose: () => void }) {
  const [got, setGot] = useState<Read<Devices> | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<Note | null>(null);
  const [tested, setTested] = useState<Tested | null>(null);
  const [now, setNow] = useState(Date.now);
  const last = useRef<Devices | null>(null);

  const take = (r: Read<Devices>) => {
    if (r.ok) {
      const paired = justPaired(last.current, r.value);
      const was = last.current?.pairing;
      if (paired) setNote({ text: `Paired ${paired.name}.`, ok: true });
      else if (was && !r.value.pairing)
        setNote(
          was.expiresAt > Date.now()
            ? { text: "The code no longer pairs: too many wrong codes were typed for it.", ok: false }
            : EXPIRED,
        );
      last.current = r.value;
    }
    setGot((was) => (r.ok || !was?.ok ? r : was));
  };

  useEffect(() => {
    let live = true;
    const read = () => void api.devices().then((r) => live && take(r));
    read();
    const poll = setInterval(read, POLL_MS);
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => {
      live = false;
      clearInterval(poll);
      clearInterval(tick);
    };
  }, []);

  const act = async (what: string, call: Promise<Read<Devices>>) => {
    setBusy(what);
    setError(null);
    setNote(null);
    setTested(null);
    const r = await call;
    setBusy(null);
    if (r.ok) take(r);
    else setError(r.error);
  };

  return (
    <PhonesView
      got={got}
      now={now}
      busy={busy}
      error={error}
      note={note}
      port={location.port || (location.protocol === "https:" ? "443" : "80")}
      onPair={() => void act("pair", api.pairDevice())}
      onRemove={(id) => void act(id, api.removeDevice(id))}
      tested={tested}
      onTest={(id) =>
        void (async () => {
          setBusy(`test:${id}`);
          setTested(null);
          const r = await api.testDevice(id);
          setBusy(null);
          setTested(r.ok ? { id, text: r.value.said, ok: true } : { id, text: r.error, ok: false });
          // A test can find notifications gone, which the list then says.
          void api.devices().then(take);
        })()
      }
      onClose={onClose}
    />
  );
}

/// The panel as drawn from what was read: `busy` is `pair`, the id of the
/// device being removed, or `test:` and the id of the one sent a test.
export function PhonesView({
  got,
  now,
  busy,
  error,
  note,
  port,
  onPair,
  onRemove,
  tested = null,
  onTest = () => {},
  onClose,
}: {
  got: Read<Devices> | null;
  now: number;
  busy: string | null;
  error: string | null;
  note: Note | null;
  port: string;
  onPair: () => void;
  onRemove: (id: string) => void;
  tested?: Tested | null;
  onTest?: (id: string) => void;
  onClose: () => void;
}) {
  const d = got?.ok ? got.value : null;
  const pairing = d?.pairing && d.pairing.expiresAt > now ? d.pairing : null;
  const size = (pairing?.qr.length ?? 0) + 8;
  const said = note ?? (d?.pairing && !pairing ? EXPIRED : null);
  return (
    <div className="phones-body">
      <div className="phones-head">
        <h2 id="phones-title">Phones</h2>
        <button type="button" className="btn phones-close" aria-label="Close" onClick={onClose}>
          <Icon name="close" size={18} />
        </button>
      </div>
      <p className="muted">A phone on your tailnet reaches this page once it is paired here.</p>
      {got && !got.ok ? (
        <p className="error">{got.error}</p>
      ) : !d ? (
        <p className="muted">Reading the devices…</p>
      ) : !d.tailnet ? (
        <p className="phones-off">Tailscale is not running on this Mac, so no phone can reach this page.</p>
      ) : pairing ? (
        <div className="pairing">
          <svg className="qr" viewBox={`0 0 ${size} ${size}`} shapeRendering="crispEdges" role="img" aria-label={`QR code of ${pairing.url}`}>
            <rect width={size} height={size} className="qr-light" />
            <path d={qrPath(pairing.qr)} className="qr-dark" />
          </svg>
          <div className="pairing-say">
            <p>Scan the code with the phone's camera, or open the link on it:</p>
            <p className="pairing-url">
              {/* A long link breaks after its host rather than at a hyphen in it. */}
              {pairing.url.slice(0, pairing.url.indexOf("/pair/") + 1)}
              <wbr />
              {pairing.url.slice(pairing.url.indexOf("/pair/") + 1)}
            </p>
            <p>
              Or type <strong className="pairing-code">{pairing.code}</strong> on the page the phone shows.
            </p>
            <p className="quiet">
              Pairs one device, for {countdown(pairing.expiresAt - now)} more.
            </p>
          </div>
        </div>
      ) : (
        <div className="pair-row">
          <button type="button" className="btn primary" disabled={busy === "pair"} onClick={onPair}>
            Pair a phone
          </button>
        </div>
      )}
      {said && (
        <p className={said.ok ? "ok" : "muted"} role="status">
          {said.text}
        </p>
      )}
      {error && <p className="error" role="alert">{error}</p>}
      {d && (
        <section className="paired" aria-labelledby="paired-title">
          <h3 id="paired-title" className="caps">
            Paired <span className="quiet">{d.devices.length}</span>
          </h3>
          {d.devices.length === 0 ? (
            <p className="quiet">No device is paired yet.</p>
          ) : (
            <ul>
              {d.devices.map((dev) => (
                <li key={dev.id}>
                  <div className="device">
                    <span className="device-name">
                      {dev.name}
                      <span className={dev.push ? "device-push on" : "device-push"}>
                        <span aria-hidden="true" className={`sdot ${dev.push ? "green" : "hollow"}`} />
                        {dev.push ? "Notifications on" : "Notifications off"}
                      </span>
                    </span>
                    <span className="quiet">
                      {dev.login} · paired {when(dev.pairedAt, now)} · used {ago(dev.lastUsed, now)}
                    </span>
                    {tested?.id === dev.id && (
                      <span className={tested.ok ? "ok" : "error"} role="status">
                        {tested.text}
                      </span>
                    )}
                  </div>
                  {dev.push && (
                    <button
                      type="button"
                      className="btn"
                      disabled={busy === `test:${dev.id}`}
                      title={`Sends ${dev.name} a notification, as a session that needs you does`}
                      onClick={() => onTest(dev.id)}
                    >
                      Send a test
                    </button>
                  )}
                  <button type="button" className="btn" disabled={busy === dev.id} onClick={() => onRemove(dev.id)}>
                    Remove
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>
      )}
      {d?.tailnet && (
        <p className="quiet phones-foot">
          Phones reach it at <span className="phones-host">https://{d.tailnet}/</span> once{" "}
          <code>tailscale serve --bg {port}</code> has run on this Mac.
        </p>
      )}
    </div>
  );
}
