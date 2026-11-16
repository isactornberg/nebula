import { useEffect, useRef, useState } from "react";
import { api } from "./api/client";
import type { PushSubscription as Kept } from "./api/contract";
import { parseRoute, type Route } from "./model";

/// What this device's notifications are: being read, impossible here until
/// the page is a Home Screen app or at all, blocked, off or on.
export type NotifyState = "reading" | "install" | "unsupported" | "denied" | "off" | "on";

export interface Browser {
  /// It has service workers, push and notifications.
  push: boolean;
  ios: boolean;
  /// The page is open as an app of its own, from the Home Screen.
  standalone: boolean;
  permission: NotificationPermission | null;
}

/// What a browser like `b` lets the page do about notifications before it
/// asks the server: iOS shows them only from the Home Screen app.
export function canNotify(b: Browser): "install" | "unsupported" | "denied" | "ask" {
  if (b.ios && !b.standalone) return "install";
  if (!b.push) return "unsupported";
  if (b.permission === "denied") return "denied";
  return "ask";
}

function browser(): Browser {
  const push = "serviceWorker" in navigator && "PushManager" in window && "Notification" in window;
  const ios = /iPhone|iPad|iPod/.test(navigator.userAgent) || (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);
  const standalone =
    matchMedia("(display-mode: standalone)").matches || (navigator as { standalone?: boolean }).standalone === true;
  return { push, ios, standalone, permission: "Notification" in window ? Notification.permission : null };
}

/// `key`, base64url, as the bytes a subscription is made with.
export function keyBytes(key: string): Uint8Array<ArrayBuffer> {
  const b64 = key.replace(/-/g, "+").replace(/_/g, "/");
  return Uint8Array.from(atob(b64 + "=".repeat((4 - (b64.length % 4)) % 4)), (c) => c.charCodeAt(0));
}

function kept(sub: PushSubscription): Kept {
  const json = sub.toJSON();
  return { endpoint: sub.endpoint, keys: { p256dh: json.keys?.p256dh ?? "", auth: json.keys?.auth ?? "" } };
}

function madeWith(sub: PushSubscription, key: string): boolean {
  const made = sub.options.applicationServerKey;
  if (!made) return false;
  const want = keyBytes(key);
  const got = new Uint8Array(made);
  return got.length === want.length && got.every((b, i) => b === want[i]);
}

function failed(e: unknown): string {
  return `This browser did not turn them on: ${e instanceof Error ? e.message : String(e)}`;
}

/// What this device's notifications are now, with the server kept to what
/// the browser holds: a subscription the browser has is kept again, in
/// case its endpoint changed, and one it no longer has, or no longer may
/// use, is dropped.
async function read(): Promise<{ state: NotifyState; key: string | null; error: string | null }> {
  const b = browser();
  const can = canNotify(b);
  if (can === "install" || can === "unsupported") return { state: can, key: null, error: null };
  const blocked = can === "denied" ? "denied" : "off";
  try {
    const reg = await navigator.serviceWorker.register("/sw.js");
    const got = await api.push();
    if (!got.ok) return { state: blocked, key: null, error: got.error };
    const { key, on } = got.value;
    let sub = await reg.pushManager.getSubscription();
    if (sub && !madeWith(sub, key)) {
      await sub.unsubscribe();
      sub = null;
    }
    if (b.permission !== "granted" || !sub) {
      if (on) await api.unsubscribe();
      return { state: blocked, key, error: null };
    }
    const again = await api.subscribe(kept(sub));
    return again.ok ? { state: "on", key, error: null } : { state: "off", key, error: again.error };
  } catch (e) {
    return { state: blocked, key: null, error: failed(e) };
  }
}

const BELL = "M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9";
const CLAPPER = "M13.73 21a2 2 0 0 1-3.46 0";

/// The bell in a paired device's top bar, and the panel it opens that
/// turns this device's notifications on or off. It registers the service
/// worker, and opens the session a tapped notification is about.
export function Notify({ onGo }: { onGo: (to: Route) => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [state, setState] = useState<NotifyState>("reading");
  const [key, setKey] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    const look = () =>
      void read().then((r) => {
        if (!live) return;
        setState(r.state);
        setKey(r.key);
        setError(r.error);
      });
    look();
    // Back from Settings, where blocked notifications are allowed.
    const shown = () => document.visibilityState === "visible" && look();
    document.addEventListener("visibilitychange", shown);
    return () => {
      live = false;
      document.removeEventListener("visibilitychange", shown);
    };
  }, []);

  useEffect(() => {
    const sw = navigator.serviceWorker;
    if (!sw) return;
    const heard = (e: MessageEvent<{ open?: unknown }>) => {
      if (typeof e.data?.open === "string") onGo(parseRoute(e.data.open));
    };
    sw.addEventListener("message", heard);
    sw.startMessages();
    return () => sw.removeEventListener("message", heard);
  }, [onGo]);

  const turnOn = () => {
    if (!key) return;
    setBusy(true);
    setError(null);
    // iOS asks only from a tap, so the question comes before anything
    // else is waited on.
    const asked = Notification.requestPermission();
    void (async () => {
      try {
        const permission = await asked;
        if (permission !== "granted") return setState(permission === "denied" ? "denied" : "off");
        const reg = await navigator.serviceWorker.ready;
        const sub = await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: keyBytes(key) });
        const got = await api.subscribe(kept(sub));
        if (got.ok) return setState("on");
        await sub.unsubscribe();
        setError(got.error);
      } catch (e) {
        setError(failed(e));
      } finally {
        setBusy(false);
      }
    })();
  };

  const turnOff = () => {
    setBusy(true);
    setError(null);
    void (async () => {
      try {
        const sub = await (await navigator.serviceWorker.ready).pushManager.getSubscription();
        await sub?.unsubscribe();
      } catch {
        // The server's copy goes either way, so nothing is sent to it.
      }
      const got = await api.unsubscribe();
      setBusy(false);
      if (got.ok) setState("off");
      else setError(got.error);
    })();
  };

  const on = state === "on";
  const label = on ? "on" : state === "denied" ? "blocked" : "off";
  return (
    <>
      <button
        type="button"
        className="btn notify-open"
        data-state={state}
        aria-haspopup="dialog"
        aria-label={`Notifications: ${label}`}
        title={`Notifications: ${label}`}
        onClick={() => dialog.current?.showModal()}
      >
        <svg
          aria-hidden="true"
          width="16"
          height="16"
          viewBox="0 0 24 24"
          fill="none"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d={BELL} />
          <path d={CLAPPER} />
          {!on && <line x1="3" y1="3" x2="21" y2="21" />}
        </svg>
      </button>
      <dialog
        ref={dialog}
        className="phones notify"
        aria-labelledby="notify-title"
        onKeyDown={(e) => e.stopPropagation()}
        onClick={(e) => e.target === dialog.current && dialog.current.close()}
      >
        <NotifyView
          state={state}
          busy={busy || (state === "off" && !key)}
          error={error}
          ios={browser().ios}
          onOn={turnOn}
          onOff={turnOff}
          onClose={() => dialog.current?.close()}
        />
      </dialog>
    </>
  );
}

/// The panel as drawn from what was read.
export function NotifyView({
  state,
  busy,
  error,
  ios,
  onOn,
  onOff,
  onClose,
}: {
  state: NotifyState;
  busy: boolean;
  error: string | null;
  ios: boolean;
  onOn: () => void;
  onOff: () => void;
  onClose: () => void;
}) {
  return (
    <div className="phones-body">
      <div className="phones-head">
        <h2 id="notify-title">Notifications</h2>
        <button type="button" className="btn close" aria-label="Close" onClick={onClose}>
          <svg aria-hidden="true" width="14" height="14" viewBox="0 0 24 24" fill="none" strokeWidth="2" strokeLinecap="round">
            <line x1="6" y1="6" x2="18" y2="18" />
            <line x1="18" y1="6" x2="6" y2="18" />
          </svg>
        </button>
      </div>
      <p className="muted">
        This phone is told when a session needs you, once your Mac has had no keyboard or mouse input for a minute.
      </p>
      {state === "reading" ? (
        <p className="muted">Reading…</p>
      ) : state === "install" ? (
        <div className="notify-state">
          <p>iOS shows notifications only from the Home Screen app.</p>
          <p className="muted">
            In Safari, tap Share, then Add to Home Screen, and turn them on in the nebula it adds.
          </p>
        </div>
      ) : state === "unsupported" ? (
        <p className="notify-state">This browser cannot show notifications from a web page.</p>
      ) : state === "denied" ? (
        <div className="notify-state">
          <p className="notify-blocked">Notifications are blocked for nebula on this phone.</p>
          <p className="muted">
            {ios
              ? "To allow them, open Settings, then Notifications, then nebula, and turn on Allow Notifications. Then come back here."
              : "To allow them, allow notifications for this page in the browser's site settings. Then come back here."}
          </p>
        </div>
      ) : (
        <div className="notify-state notify-row">
          <p>
            <span aria-hidden="true" className={`dot ${state === "on" ? "tone-green" : "tone-hollow"}`} />
            {state === "on" ? "On for this phone." : "Off for this phone."}
          </p>
          {state === "on" ? (
            <button type="button" className="btn" disabled={busy} onClick={onOff}>
              Turn off
            </button>
          ) : (
            <button type="button" className="btn primary" disabled={busy} onClick={onOn}>
              Turn on notifications
            </button>
          )}
        </div>
      )}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
