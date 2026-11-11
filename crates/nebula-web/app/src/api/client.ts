import { useEffect, useState } from "react";
import type { ApiError, Screen, SessionDetail, Shown, SkillDetail, Skills, State } from "./contract";

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

/// What a read answered: its body, or what went wrong.
export type Read<T> = { ok: true; value: T } | { ok: false; error: string };

async function read<T>(path: string, signal: AbortSignal): Promise<Read<T>> {
  try {
    const res = await fetch(path, { signal });
    if (res.ok) return { ok: true, value: (await res.json()) as T };
    const err = (await res.json().catch(() => null)) as ApiError | null;
    return { ok: false, error: err?.error ?? `The server answered ${res.status}.` };
  } catch {
    return { ok: false, error: "The server did not answer." };
  }
}

/// The skills on this machine, read when the page shows them and again
/// whenever the window comes back into view, since they change on disk.
export function useSkills(): Read<Skills> | null {
  const [got, setGot] = useState<Read<Skills> | null>(null);
  useEffect(() => {
    const ctl = new AbortController();
    const load = () => {
      if (document.hidden) return;
      void read<Skills>("/api/skills", ctl.signal).then((r) => ctl.signal.aborted || setGot(r));
    };
    load();
    addEventListener("focus", load);
    document.addEventListener("visibilitychange", load);
    return () => {
      ctl.abort();
      removeEventListener("focus", load);
      document.removeEventListener("visibilitychange", load);
    };
  }, []);
  return got;
}

/// Skill `id`'s detail, its file read when it opens and again whenever
/// the list is, `list` being the one read last.
export function useSkillDetail(id: string | null, list: unknown): Read<SkillDetail> | null {
  const [got, setGot] = useState<{ id: string; read: Read<SkillDetail> } | null>(null);
  useEffect(() => {
    if (!id) return;
    const ctl = new AbortController();
    void read<SkillDetail>(`/api/skills/${enc(id)}`, ctl.signal).then((r) => ctl.signal.aborted || setGot({ id, read: r }));
    return () => ctl.abort();
  }, [id, list]);
  return got && got.id === id ? got.read : null;
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
