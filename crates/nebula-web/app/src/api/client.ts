import { useEffect, useState } from "react";
import type { ApiError, Screen, SessionDetail, Shown, State } from "./contract";

const enc = encodeURIComponent;

/// The state as the server pushes it, and whether the stream is up. The
/// browser retries a dropped stream on its own; one it gave up on (a
/// refused or failed response) is opened again here.
export function useLiveState(): { state: State | null; connected: boolean } {
  const [state, setState] = useState<State | null>(null);
  const [connected, setConnected] = useState(true);
  useEffect(() => {
    let es: EventSource | null = null;
    let retry: ReturnType<typeof setTimeout> | undefined;
    const connect = () => {
      es = new EventSource("/api/events");
      es.addEventListener("state", (e) => {
        setState(JSON.parse((e as MessageEvent<string>).data) as State);
        setConnected(true);
      });
      es.onerror = () => {
        setConnected(false);
        if (es?.readyState === EventSource.CLOSED) {
          es.close();
          retry = setTimeout(connect, 2000);
        }
      };
    };
    connect();
    return () => {
      clearTimeout(retry);
      es?.close();
    };
  }, []);
  return { state, connected };
}

/// How long after a turn ends Claude may still be writing its reply to the
/// transcript: the daemon waits as long for one.
const REPLY_SETTLE_MS = 3000;

/// Session `id`'s detail, read again whenever `version` changes, and once
/// more when a state that began at `since` has stood long enough for its
/// reply to be written.
export function useDetail(id: string | null, version: string, since: number): SessionDetail | null {
  const [got, setGot] = useState<SessionDetail | null>(null);
  useEffect(() => {
    if (!id) return;
    const ctl = new AbortController();
    const read = () =>
      fetch(`/api/sessions/${enc(id)}`, { signal: ctl.signal })
        .then((r) => (r.ok ? (r.json() as Promise<SessionDetail>) : null))
        .then((d) => d && setGot(d))
        .catch(() => {});
    void read();
    const settle = since + REPLY_SETTLE_MS - Date.now();
    const again = settle > 0 ? setTimeout(read, settle + 250) : undefined;
    return () => {
      clearTimeout(again);
      ctl.abort();
    };
  }, [id, version, since]);
  return got && got.session.id === id ? got : null;
}

/// Session `id`'s screen, read once a second while shown.
export function useScreen(id: string): Screen | null {
  const [screen, setScreen] = useState<Screen | null>(null);
  useEffect(() => {
    const ctl = new AbortController();
    const read = () => {
      if (document.hidden) return;
      fetch(`/api/sessions/${enc(id)}/screen`, { signal: ctl.signal })
        .then((r) => (r.ok ? (r.json() as Promise<Screen>) : null))
        .then((s) => s && setScreen(s))
        .catch(() => {});
    };
    read();
    const timer = setInterval(read, 1000);
    return () => {
      clearInterval(timer);
      ctl.abort();
    };
  }, [id]);
  return screen;
}

/// Posts `body` to `path`: the response when accepted, else what went wrong.
async function act(path: string, body?: unknown): Promise<Response | string> {
  try {
    const res = await fetch(path, {
      method: "POST",
      headers: body === undefined ? undefined : { "Content-Type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (res.ok) return res;
    const err = (await res.json().catch(() => null)) as ApiError | null;
    return err?.error ?? `The server answered ${res.status}.`;
  } catch {
    return "The server did not answer.";
  }
}

/// Posts `body` to `path`: null when accepted, else what went wrong.
async function post(path: string, body?: unknown): Promise<string | null> {
  const got = await act(path, body);
  return typeof got === "string" ? got : null;
}

async function show(id: string): Promise<Shown | ApiError> {
  const got = await act(`/api/sessions/${enc(id)}/show`);
  if (typeof got === "string") return { error: got };
  return ((await got.json().catch(() => null)) as Shown | null) ?? { alreadyOnScreen: false };
}

export const api = {
  send: (id: string, text: string) => post(`/api/sessions/${enc(id)}/send`, { text }),
  answer: (id: string, option: number) => post(`/api/sessions/${enc(id)}/answer`, { option }),
  show,
  seen: (id: string) => post(`/api/sessions/${enc(id)}/seen`),
  orchestrator: (text: string) => post("/api/orchestrator/send", { text }),
};
