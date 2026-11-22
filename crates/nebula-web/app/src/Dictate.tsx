import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { isIos } from "./Notify";
import { focusBack } from "./palette";
import { arrowsMove, Icon } from "./ui";

/// The languages dictation listens in.
export type Lang = "en" | "sv";
export const LANGS: { id: Lang; label: string }[] = [
  { id: "en", label: "English" },
  { id: "sv", label: "Svenska" },
];

const LANG_KEY = "nebula.dictation";

/// What the mic says where the browser cannot listen, or was not let.
export const KEYBOARD_MIC = "Dictation is not available here: use the keyboard's mic.";

type Store = Pick<Storage, "getItem" | "setItem">;

function store(): Store | undefined {
  try {
    return globalThis.localStorage;
  } catch {
    return undefined;
  }
}

function browserLangs(): readonly string[] {
  return typeof navigator === "undefined" ? [] : (navigator.languages ?? [navigator.language]);
}

/// The language this device picked last, else the first of the browser's
/// that is English or Swedish, else English.
export function readLang(kept: Store | undefined, browser: readonly string[]): Lang {
  try {
    const got = kept?.getItem(LANG_KEY);
    if (got === "en" || got === "sv") return got;
  } catch {
    // Not kept on this device, then.
  }
  const first = browser.find((l) => /^(en|sv)(-|$)/i.test(l));
  return first?.toLowerCase().startsWith("sv") ? "sv" : "en";
}

export function keepLang(kept: Store | undefined, lang: Lang) {
  try {
    kept?.setItem(LANG_KEY, lang);
  } catch {
    // Not remembered across reloads, then.
  }
}

/// The tag a recognizer listens for: the browser's own English when it has
/// one, `en-GB`, else US English; Swedish as Sweden speaks it.
export function langTag(lang: Lang, browser: readonly string[]): string {
  if (lang === "sv") return "sv-SE";
  return browser.find((l) => /^en-/i.test(l)) ?? "en-US";
}

/// The Web Speech API's recognizer, as much of it as dictation uses.
export interface Recognizer {
  lang: string;
  continuous: boolean;
  interimResults: boolean;
  onresult: ((e: { results: ArrayLike<ArrayLike<{ transcript: string }> & { isFinal: boolean }> }) => void) | null;
  onerror: ((e: { error: string }) => void) | null;
  onend: (() => void) | null;
  start(): void;
  stop(): void;
  abort(): void;
}

export type RecognizerClass = new () => Recognizer;

export function recognizerClass(): RecognizerClass | null {
  const w = globalThis as { SpeechRecognition?: RecognizerClass; webkitSpeechRecognition?: RecognizerClass };
  return w.SpeechRecognition ?? w.webkitSpeechRecognition ?? null;
}

/// The words heard so far, and those still being guessed at.
export type Heard = { said: string; guess: string };

const NOTHING: Heard = { said: "", guess: "" };

/// How a dictation ended: its words, whether they go as heard, and what
/// went wrong, if anything. `refused` is the browser or the user saying no.
export type Ended = { words: string; send: boolean; problem: string | null; refused: boolean };

const join = (...parts: string[]) =>
  parts
    .map((p) => p.trim())
    .filter(Boolean)
    .join(" ");

export const heardWords = (h: Heard) => join(h.said, h.guess);

const NOTHING_HEARD = "Nothing was heard.";

/// What iOS 27 hears once a dictation was cut short: nothing.
const DEAF = "Nothing was heard. If you spoke, iOS has lost the mic: close the app and open it again.";

function problemOf(error: string, words: string): string | null {
  switch (error) {
    case "aborted":
      return null;
    case "no-speech":
      return words ? null : NOTHING_HEARD;
    case "network":
      return "Dictation needs the network, which did not answer.";
    case "audio-capture":
      return "No microphone was found.";
    case "language-not-supported":
      return "This browser cannot listen in that language.";
    default:
      return `Dictation stopped: ${error}.`;
  }
}

/// How long a recognizer asked to stop may take to say it has ended.
const END_MS = 2000;

/// How long one let go of on iOS may listen on before it is cut short.
const DRAIN_MS = 8000;

/// Says a cue in `tag` and calls `done` once it has been said, or at once
/// where it cannot be; what it gives back stops it.
export type Cue = (tag: string, done: () => void) => () => void;

/// How long a cue may take before listening starts anyway.
const CUE_MS = 3000;

/// Says "Listening" in the language listened in.
export const speakCue: Cue = (tag, done) => {
  const synth = globalThis.speechSynthesis;
  let over = false;
  const finish = () => {
    if (over) return;
    over = true;
    clearTimeout(timer);
    done();
  };
  const timer = setTimeout(() => {
    synth?.cancel();
    finish();
  }, CUE_MS);
  if (!synth) {
    finish();
    return () => {};
  }
  const u = new SpeechSynthesisUtterance(tag.startsWith("sv") ? "Lyssnar" : "Listening");
  u.lang = tag;
  u.onend = u.onerror = finish;
  // A queue an utterance left stuck says nothing more until cancelled.
  if (synth.speaking || synth.pending) synth.cancel();
  synth.speak(u);
  return () => {
    over = true;
    clearTimeout(timer);
    synth.cancel();
  };
};

/// iOS: how many recognizers let go of have yet to end, and what waits for
/// them to.
let ending = 0;
let waiting: (() => void)[] = [];

/// iOS: leaves `r` to end by itself once the speaker pauses, cut short
/// only when it has heard nothing, as then it never would, or listens on
/// too long.
function letGo(r: Recognizer, heard: boolean) {
  ending++;
  let gone = false;
  const done = () => {
    if (gone) return;
    gone = true;
    clearTimeout(timer);
    r.onend = null;
    if (--ending > 0) return;
    const go = waiting;
    waiting = [];
    go.forEach((g) => g());
  };
  const timer = setTimeout(
    () => {
      r.abort();
      done();
    },
    heard ? DRAIN_MS : END_MS,
  );
  r.onresult = r.onerror = null;
  r.onend = done;
  if (!heard) r.abort();
}

function afterLetGo(go: () => void) {
  if (ending === 0) go();
  else waiting.push(go);
}

/// One dictation: a recognizer listening, its words as they come, and its
/// end, by Stop, Send, Cancel, the browser, or an error. A word still being
/// guessed when it stops is kept.
///
/// iOS 27 hears the first dictation after the app opens and, after that,
/// only one that a spoken cue ends just before and that is never cut short
/// by stop() or abort(): once one has been, every later one starts and
/// hears nothing until the app is closed (WebKit bug 321436). So with a
/// `cue` each dictation says it first, listens for one stretch of speech,
/// which ends by itself at a pause, and lets the recognizer go on Stop,
/// Send or Cancel, the next waiting until it has ended.
export class Listening {
  heard: Heard = NOTHING;
  private r: Recognizer | null = null;
  /// The words heard before the language changed.
  private kept = "";
  private wants: "stop" | "send" | "cancel" | "relang" | null = null;
  private error: string | null = null;
  private tag = "";
  private timer: ReturnType<typeof setTimeout> | undefined;
  /// The recognizer has heard words.
  private spoke = false;
  /// With a cue: stops it, or the wait before it, until listening starts.
  private hush: (() => void) | null = null;

  constructor(
    private Rec: RecognizerClass,
    private on: { heard: (h: Heard) => void; ended: (e: Ended | null) => void },
    private cue?: Cue,
  ) {}

  /// Listens in `tag`: false when the browser would not start.
  begin(tag: string): boolean {
    this.kept = "";
    this.heard = NOTHING;
    if (!this.cue) return this.listen(tag);
    this.ready(tag);
    return true;
  }

  stop() {
    this.end("stop");
  }

  send() {
    this.end("send");
  }

  cancel() {
    this.end("cancel");
  }

  /// Listens on in `tag`, the words so far kept.
  relang(tag: string) {
    if (this.wants || !(this.r || this.hush)) return;
    this.kept = heardWords(this.heard);
    this.tag = tag;
    if (this.cue) {
      this.hush?.();
      if (this.r) this.release();
      return this.ready(tag);
    }
    this.wants = "relang";
    this.r!.abort();
  }

  /// With a cue: once no recognizer let go of listens on, says it, then
  /// listens.
  private ready(tag: string) {
    let hushed = false;
    this.hush = () => {
      hushed = true;
    };
    afterLetGo(() => {
      if (hushed) return;
      let said = false;
      const hush = this.cue!(tag, () => {
        said = true;
        this.hush = null;
        if (!this.listen(tag)) this.on.ended({ words: this.kept, send: false, problem: null, refused: true });
      });
      if (!said) this.hush = hush;
    });
  }

  private listen(tag: string): boolean {
    const r = new this.Rec();
    r.lang = tag;
    r.continuous = !this.cue;
    r.interimResults = true;
    r.onresult = (e) => {
      const said: string[] = [];
      const guess: string[] = [];
      for (let i = 0; i < e.results.length; i++) {
        const result = e.results[i]!;
        (result.isFinal ? said : guess).push(result[0]?.transcript ?? "");
      }
      this.spoke ||= [...said, ...guess].some((t) => t.trim());
      this.heard = { said: join(this.kept, ...said), guess: join(...guess) };
      this.on.heard(this.heard);
    };
    r.onerror = (e) => {
      this.error = e.error;
    };
    r.onend = () => this.ended(r);
    this.r = r;
    this.wants = null;
    this.error = null;
    this.spoke = false;
    try {
      r.start();
      return true;
    } catch {
      this.r = null;
      return false;
    }
  }

  private end(why: "stop" | "send" | "cancel") {
    const r = this.r;
    if (this.wants && this.wants !== "relang") return;
    if (this.cue) {
      if (!r && !this.hush) return;
      this.wants = why;
      this.hush?.();
      this.hush = null;
      if (r) this.release();
      return this.finish();
    }
    if (!r) return;
    this.wants = why;
    if (why === "cancel") r.abort();
    else r.stop();
    clearTimeout(this.timer);
    this.timer = setTimeout(() => this.ended(r), END_MS);
  }

  /// With a cue: leaves the recognizer to end by itself.
  private release() {
    letGo(this.r!, this.spoke);
    this.r = null;
  }

  private ended(r: Recognizer) {
    if (r !== this.r) return;
    clearTimeout(this.timer);
    r.onresult = r.onerror = r.onend = null;
    this.r = null;
    if (this.wants === "relang") {
      if (this.listen(this.tag)) return;
      this.wants = "stop";
    }
    this.finish();
  }

  private finish() {
    if (this.wants === "cancel") return this.on.ended(null);
    const words = heardWords(this.heard);
    const refused = this.error === "not-allowed" || this.error === "service-not-allowed";
    const problem = refused ? null : (this.error && problemOf(this.error, words)) || (words ? null : this.cue ? DEAF : NOTHING_HEARD);
    this.on.ended({ words, send: this.wants === "send" && !refused && problem === null, problem, refused });
  }
}

/// A dictation listening in `tag`, or null where it cannot: the browser has
/// no recognizer, it was refused before, or it would not start. With a
/// `cue`, as on iOS, said before it listens.
export function startListening(
  Rec: RecognizerClass | null,
  refused: boolean,
  tag: string,
  on: { heard: (h: Heard) => void; ended: (e: Ended | null) => void },
  cue?: Cue,
): Listening | null {
  if (!Rec || refused) return null;
  const l = new Listening(Rec, on, cue);
  return l.begin(tag) ? l : null;
}

export interface Dictation {
  /// Listening now.
  on: boolean;
  heard: Heard;
  lang: Lang;
  /// When it began listening.
  since: number;
  begin: () => void;
  stop: () => void;
  send: () => void;
  cancel: () => void;
  pick: (lang: Lang) => void;
}

/// Dictation into a box: `ended` gets what was heard, or that the browser
/// refused; `unavailable` is called in place of listening where it cannot.
export function useDictation(ended: (e: Ended) => void, unavailable: () => void): Dictation {
  const [lang, setLang] = useState<Lang>(() => readLang(store(), browserLangs()));
  const [on, setOn] = useState(false);
  const [heard, setHeard] = useState<Heard>(NOTHING);
  const [since, setSince] = useState(0);
  const listening = useRef<Listening | null>(null);
  const refused = useRef(false);
  const said = useRef({ ended, unavailable });
  said.current = { ended, unavailable };
  useEffect(() => () => listening.current?.cancel(), []);

  const begin = () => {
    if (listening.current) return;
    const l = startListening(
      recognizerClass(),
      refused.current,
      langTag(lang, browserLangs()),
      {
        heard: setHeard,
        ended: (e) => {
          listening.current = null;
          setOn(false);
          setHeard(NOTHING);
          if (!e) return;
          if (e.refused) {
            refused.current = true;
            said.current.unavailable();
          } else said.current.ended(e);
        },
      },
      isIos() ? speakCue : undefined,
    );
    if (!l) return said.current.unavailable();
    listening.current = l;
    setHeard(NOTHING);
    setSince(Date.now());
    setOn(true);
  };
  const pick = (to: Lang) => {
    setLang(to);
    keepLang(store(), to);
    listening.current?.relang(langTag(to, browserLangs()));
  };
  return {
    on,
    heard,
    lang,
    since,
    begin,
    stop: () => listening.current?.stop(),
    send: () => listening.current?.send(),
    cancel: () => listening.current?.cancel(),
    pick,
  };
}

/// `0:07`.
export function elapsed(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

function useElapsed(since: number): string {
  const [now, setNow] = useState(since);
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [since]);
  return elapsed(now - since);
}

/// Listening: the words as they come, those still guessed at faint, how
/// long it has listened, the language, and Cancel, Stop and Send. On a
/// phone a modal sheet over the thread, which gives the focus back once it
/// closes; on a wider screen in the composer's place. A screen reader hears
/// the words once they are final, never each guess on the way.
export function DictatePanel({
  d,
  to,
  sheet,
  canSend,
}: {
  d: Dictation;
  /// `Replying to the captain · GPS link recovery`.
  to: string;
  sheet: boolean;
  canSend: boolean;
}) {
  const time = useElapsed(d.since);
  const stop = useRef<HTMLButtonElement>(null);
  const modal = useRef<HTMLDialogElement>(null);
  useLayoutEffect(() => {
    const dialog = modal.current;
    if (!dialog) return;
    const back = document.activeElement as HTMLElement | null;
    dialog.showModal();
    return () => {
      dialog.close();
      focusBack(back);
    };
  }, [sheet]);
  useEffect(() => stop.current?.focus(), [sheet]);
  const keys = (e: KeyboardEvent) => {
    if (e.key !== "Escape") return;
    e.preventDefault();
    e.stopPropagation();
    d.cancel();
  };
  const body = (
    <>
      {sheet && <span aria-hidden="true" className="dict-grip" />}
      <div className="dict-head">
        <span aria-hidden="true" className="dict-meter">
          <span />
          <span />
          <span />
          <span />
          <span />
        </span>
        <span className="dict-word">Listening</span>
        <span className="dict-time">{time}</span>
        <div role="radiogroup" aria-label="Language" className="dict-langs" onKeyDown={arrowsMove}>
          {LANGS.map((l) => (
            <button
              key={l.id}
              type="button"
              role="radio"
              aria-checked={d.lang === l.id}
              tabIndex={d.lang === l.id ? 0 : -1}
              onClick={() => d.lang !== l.id && d.pick(l.id)}
            >
              {l.label}
            </button>
          ))}
        </div>
      </div>
      <span className="dict-to">{to}</span>
      <p className="dict-words">
        <span aria-live="polite">{d.heard.said}</span>
        {d.heard.said && d.heard.guess && " "}
        {d.heard.guess && <span className="dict-guess">{d.heard.guess}</span>}
        <span aria-hidden="true" className="dict-caret" />
        {!d.heard.said && !d.heard.guess && <span className="dict-wait">Speak, and your words show here.</span>}
      </p>
      <div className="dict-acts">
        <button type="button" className="dict-cancel" onClick={d.cancel}>
          Cancel
        </button>
        <button ref={stop} type="button" className="dict-stop" aria-label="Stop, and edit the words" title="Stop, and edit the words" onClick={d.stop}>
          <span />
        </button>
        <button type="button" className="dict-send" disabled={!canSend} onClick={d.send}>
          Send
          <Icon name="arrowUp" size={17} />
        </button>
      </div>
    </>
  );
  if (!sheet) {
    return (
      <div className="dict" role="group" aria-label="Dictating" onKeyDown={keys}>
        {body}
      </div>
    );
  }
  return (
    <dialog
      ref={modal}
      className="dict sheet"
      aria-label={`Dictating, ${to}`}
      onKeyDown={keys}
      onCancel={(e) => {
        e.preventDefault();
        d.cancel();
      }}
      // Chrome closes it on its own on an Esc it no longer lets be cancelled.
      onClose={(e) => !e.currentTarget.open && d.cancel()}
      // A tap above the sheet, on the page dimmed behind it, cancels.
      onClick={(e) => e.target === e.currentTarget && e.clientY < e.currentTarget.getBoundingClientRect().top && d.cancel()}
    >
      {body}
    </dialog>
  );
}
