import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  DictatePanel,
  elapsed,
  keepLang,
  langTag,
  readLang,
  recognizerClass,
  startListening,
  type Cue,
  type Dictation,
  type Ended,
  type Heard,
  type Recognizer,
} from "./Dictate";

/// A recognizer as the Web Speech API's, driven by the test.
class FakeRecognizer implements Recognizer {
  static made: FakeRecognizer[] = [];
  static failsToStart = false;
  lang = "";
  continuous = false;
  interimResults = false;
  onresult: Recognizer["onresult"] = null;
  onerror: Recognizer["onerror"] = null;
  onend: Recognizer["onend"] = null;
  calls: string[] = [];
  constructor() {
    FakeRecognizer.made.push(this);
  }
  start() {
    if (FakeRecognizer.failsToStart) throw new Error("InvalidStateError");
    this.calls.push("start");
  }
  stop() {
    this.calls.push("stop");
  }
  abort() {
    this.calls.push("abort");
  }
  /// The results so far: each a phrase, final or still guessed at.
  hear(...results: [string, boolean][]) {
    this.onresult?.({ results: results.map(([transcript, isFinal]) => Object.assign([{ transcript }], { isFinal })) });
  }
  fail(error: string) {
    this.onerror?.({ error });
  }
  end() {
    this.onend?.();
  }
}

/// iOS 27's recognizer, as the reports on WebKit bug 321436 have it: the
/// first dictation after the app opens hears. Once one has been cut short by
/// stop() or abort(), or one starts without a spoken cue ending just before
/// it, every later one starts as ever and never hears a word. One that is
/// not continuous ends by itself when the speaker pauses. Its events come a
/// task later, as WebKit's do.
class IosRecognizer implements Recognizer {
  static page = { started: 0, cut: false, cued: false, live: null as IosRecognizer | null };
  static made: IosRecognizer[] = [];
  lang = "";
  continuous = false;
  interimResults = false;
  onresult: Recognizer["onresult"] = null;
  onerror: Recognizer["onerror"] = null;
  onend: Recognizer["onend"] = null;
  deaf = false;
  ended = false;
  words = "";
  constructor() {
    IosRecognizer.made.push(this);
  }
  start() {
    const page = IosRecognizer.page;
    // WebKit aborts the one listening when another starts.
    page.live?.cut();
    this.deaf = page.cut || (page.started > 0 && !page.cued);
    page.started++;
    page.cued = false;
    page.live = this;
  }
  stop() {
    this.cut();
  }
  abort() {
    this.cut();
  }
  cut() {
    if (this.ended) return;
    IosRecognizer.page.cut = true;
    if (this.words) this.result(true);
    this.end(this.words ? null : "aborted");
  }
  /// The speaker says `words` while this one listens.
  hears(words: string) {
    if (this.deaf || this.ended) return;
    this.words = [this.words, words].filter(Boolean).join(" ");
    this.result(false);
  }
  /// The speaker pauses.
  pause() {
    if (this.deaf || this.ended || !this.words) return;
    this.result(true);
    if (!this.continuous) this.end(null);
  }
  private result(isFinal: boolean) {
    const transcript = this.words;
    queueMicrotask(() => this.onresult?.({ results: [Object.assign([{ transcript }], { isFinal })] }));
  }
  private end(error: string | null) {
    this.ended = true;
    if (IosRecognizer.page.live === this) IosRecognizer.page.live = null;
    queueMicrotask(() => {
      if (error) this.onerror?.({ error });
      this.onend?.();
    });
  }
}

/// The cue iOS says before it listens, which the test ends.
const said = { tags: [] as string[], done: null as (() => void) | null, hushed: 0 };
const iosCue: Cue = (tag, done) => {
  said.tags.push(tag);
  said.done = () => {
    said.done = null;
    IosRecognizer.page.cued = true;
    done();
  };
  return () => {
    said.hushed++;
    said.done = null;
  };
};

const settle = async () => {
  for (let i = 0; i < 10; i++) await Promise.resolve();
};

/// The speaker, to whichever recognizer is listening in the page.
const speak = (words: string) => IosRecognizer.page.live?.hears(words);
const pause = () => IosRecognizer.page.live?.pause();

function onIos(tag = "en-GB") {
  const heard: Heard[] = [];
  const ended: (Ended | null)[] = [];
  const l = startListening(IosRecognizer, false, tag, { heard: (h) => heard.push(h), ended: (e) => ended.push(e) }, iosCue);
  return { l: l!, heard, ended };
}

function listen(tag = "en-US") {
  const heard: Heard[] = [];
  const ended: (Ended | null)[] = [];
  const l = startListening(FakeRecognizer, false, tag, { heard: (h) => heard.push(h), ended: (e) => ended.push(e) });
  return { l: l!, r: FakeRecognizer.made.at(-1)!, heard, ended };
}

afterEach(async () => {
  FakeRecognizer.made = [];
  FakeRecognizer.failsToStart = false;
  // Ends what a test left listening, so the next starts in a quiet page.
  for (const r of IosRecognizer.made) r.cut();
  await settle();
  IosRecognizer.made = [];
  IosRecognizer.page = { started: 0, cut: false, cued: false, live: null };
  Object.assign(said, { tags: [], done: null, hushed: 0 });
  vi.useRealTimers();
});

describe("dictation on iOS", () => {
  /// Opens dictation, waits out the cue, says `words`, ends it `how` (by
  /// Stop, Send, or a pause), and falls quiet: how it ended, or that it is
  /// listening still.
  async function dictate(words: string, how: "stop" | "send" | "pause") {
    const { l, ended } = onIos();
    said.done?.();
    await settle();
    speak(words);
    await settle();
    if (how === "pause") pause();
    else l[how]();
    await settle();
    pause();
    await settle();
    return ended.length ? ended[0] : "still listening";
  }

  const put = (words: string, send = false) => ({ words, send, problem: null, refused: false });

  it("hears the first, second and third dictation, none cut short", async () => {
    const box = [
      await dictate("Go with the udev rule", "stop"),
      await dictate("keep the retry in code", "pause"),
      await dictate("and ship it", "send"),
    ];
    expect(box).toEqual([put("Go with the udev rule"), put("keep the retry in code"), put("and ship it", true)]);
    expect(IosRecognizer.made.map((r) => r.continuous)).toEqual([false, false, false]);
    expect(said.tags).toEqual(["en-GB", "en-GB", "en-GB"]);
    expect(IosRecognizer.page.cut).toBe(false);
  });

  it("listens again only once the recognizer it let go of has ended, and cuts one that listens on", async () => {
    vi.useFakeTimers();
    const first = onIos();
    said.done?.();
    speak("udev rule");
    await settle();
    first.l.stop();
    expect(first.ended).toEqual([put("udev rule")]);
    const next = onIos();
    expect(said.tags).toHaveLength(1);
    pause();
    await settle();
    expect(said.tags).toHaveLength(2);
    said.done?.();
    expect(IosRecognizer.made).toHaveLength(2);

    speak("still talking");
    await settle();
    next.l.cancel();
    expect(next.ended).toEqual([null]);
    speak("to someone else");
    vi.advanceTimersByTime(8000);
    await settle();
    expect(IosRecognizer.made[1]!.ended).toBe(true);
    expect(IosRecognizer.page.cut).toBe(true);
  });

  it("cuts a dictation stopped before a word was heard, and says how to get the mic back when the next hears nothing", async () => {
    const quiet = onIos();
    said.done?.();
    await settle();
    quiet.l.stop();
    await settle();
    expect(quiet.ended).toEqual([
      {
        words: "",
        send: false,
        problem: "Nothing was heard. If you spoke, iOS has lost the mic: close the app and open it again.",
        refused: false,
      },
    ]);
    expect(IosRecognizer.page.cut).toBe(true);
    expect((await dictate("hello", "send")) as Ended).toMatchObject({ words: "", send: false });
  });

  it("listens on in the other language, saying so first, the words so far kept", async () => {
    const { l, heard, ended } = onIos("en-GB");
    l.relang("sv-SE");
    expect(said.hushed).toBe(1);
    said.done?.();
    speak("Använd udev-regeln");
    await settle();
    l.relang("en-GB");
    expect(said.tags).toEqual(["en-GB", "sv-SE"]);
    pause();
    await settle();
    expect(said.tags).toEqual(["en-GB", "sv-SE", "en-GB"]);
    said.done?.();
    speak("and ship it");
    await settle();
    expect(heard.at(-1)).toEqual({ said: "Använd udev-regeln", guess: "and ship it" });
    pause();
    await settle();
    expect(ended).toEqual([put("Använd udev-regeln and ship it")]);
    expect(IosRecognizer.made.map((r) => r.lang)).toEqual(["sv-SE", "en-GB"]);
    expect(IosRecognizer.page.cut).toBe(false);
  });

  it("drops the words on Cancel, the recognizer left to end by itself", async () => {
    const { l, ended } = onIos();
    said.done?.();
    speak("never mind");
    await settle();
    l.cancel();
    pause();
    await settle();
    expect(ended).toEqual([null]);
    expect(IosRecognizer.page.cut).toBe(false);
  });
});

describe("dictation", () => {
  it("starts a continuous recognizer with words as they are guessed, in the language picked", () => {
    const { r } = listen("sv-SE");
    expect(r.calls).toEqual(["start"]);
    expect(r.lang).toBe("sv-SE");
    expect(r.continuous).toBe(true);
    expect(r.interimResults).toBe(true);
  });

  it("hears the words said apart from those still being guessed", () => {
    const { r, heard } = listen();
    r.hear(["Go with the udev rule.", true], [" Keep the retry", false]);
    expect(heard.at(-1)).toEqual({ said: "Go with the udev rule.", guess: "Keep the retry" });
    r.hear(["Go with the udev rule.", true], [" Keep the retry in code.", true], [" and add a", false]);
    expect(heard.at(-1)).toEqual({ said: "Go with the udev rule. Keep the retry in code.", guess: "and add a" });
  });

  it("puts the words in the box on Stop, a word still guessed at kept", () => {
    const { l, r, ended } = listen();
    r.hear(["Add a test for", true], [" the unplug", false]);
    l.stop();
    expect(r.calls).toEqual(["start", "stop"]);
    expect(ended).toEqual([]);
    r.end();
    expect(ended).toEqual([{ words: "Add a test for the unplug", send: false, problem: null, refused: false }]);
  });

  it("sends the words as heard on Send", () => {
    const { l, r, ended } = listen();
    r.hear(["udev rule", true]);
    l.send();
    r.hear(["udev rule, and ship it", true]);
    r.end();
    expect(ended).toEqual([{ words: "udev rule, and ship it", send: true, problem: null, refused: false }]);
  });

  it("ends with what it heard when the browser never says it stopped", () => {
    vi.useFakeTimers();
    const { l, r, ended } = listen();
    r.hear(["stop mid", false]);
    l.stop();
    vi.advanceTimersByTime(2000);
    expect(ended).toEqual([{ words: "stop mid", send: false, problem: null, refused: false }]);
    r.end();
    expect(ended).toHaveLength(1);
  });

  it("drops the words on Cancel", () => {
    const { l, r, ended } = listen();
    r.hear(["never mind", true]);
    l.cancel();
    expect(r.calls).toEqual(["start", "abort"]);
    r.fail("aborted");
    r.end();
    expect(ended).toEqual([null]);
  });

  it("keeps what the browser heard before it stopped on its own", () => {
    const { r, ended } = listen();
    r.hear(["a long pause", true]);
    r.end();
    expect(ended).toEqual([{ words: "a long pause", send: false, problem: null, refused: false }]);
  });

  it("says when nothing was heard, keeps the words when the network fails, and never sends them then", () => {
    const quiet = listen();
    quiet.r.fail("no-speech");
    quiet.r.end();
    expect(quiet.ended).toEqual([{ words: "", send: false, problem: "Nothing was heard.", refused: false }]);

    const stopped = listen();
    stopped.l.send();
    stopped.r.end();
    expect(stopped.ended).toEqual([{ words: "", send: false, problem: "Nothing was heard.", refused: false }]);

    const offline = listen();
    offline.r.hear(["half a thought", true]);
    offline.l.send();
    offline.r.fail("network");
    offline.r.end();
    expect(offline.ended).toEqual([
      { words: "half a thought", send: false, problem: "Dictation needs the network, which did not answer.", refused: false },
    ]);
  });

  it("is refused when the page may not use the mic", () => {
    for (const error of ["not-allowed", "service-not-allowed"]) {
      const { r, ended } = listen();
      r.fail(error);
      r.end();
      expect(ended).toEqual([{ words: "", send: false, problem: null, refused: true }]);
    }
  });

  it("listens on in the other language with the words so far", () => {
    const { l, r, heard } = listen("en-US");
    r.hear(["Use the udev rule", true]);
    l.relang("sv-SE");
    expect(r.calls).toEqual(["start", "abort"]);
    r.end();
    const next = FakeRecognizer.made.at(-1)!;
    expect(next).not.toBe(r);
    expect(next.lang).toBe("sv-SE");
    next.hear(["och skicka den", false]);
    expect(heard.at(-1)).toEqual({ said: "Use the udev rule", guess: "och skicka den" });
  });

  it("does not listen where the browser has no recognizer, was refused, or will not start", () => {
    const on = { heard: () => {}, ended: () => {} };
    expect(startListening(null, false, "en-US", on)).toBeNull();
    expect(startListening(FakeRecognizer, true, "en-US", on)).toBeNull();
    FakeRecognizer.failsToStart = true;
    expect(startListening(FakeRecognizer, false, "en-US", on)).toBeNull();
  });

  it("finds the browser's recognizer, prefixed or not, and none where it has none", () => {
    expect(recognizerClass()).toBeNull();
    vi.stubGlobal("webkitSpeechRecognition", FakeRecognizer);
    expect(recognizerClass()).toBe(FakeRecognizer);
    vi.unstubAllGlobals();
  });
});

describe("the dictation language", () => {
  const store = (kept: Record<string, string> = {}) => ({
    getItem: (k: string) => kept[k] ?? null,
    setItem: (k: string, v: string) => {
      kept[k] = v;
    },
  });

  it("is the browser's when it is English or Swedish, else English", () => {
    expect(readLang(store(), ["sv-SE", "en-US"])).toBe("sv");
    expect(readLang(store(), ["de-DE", "en-GB"])).toBe("en");
    expect(readLang(store(), ["de-DE"])).toBe("en");
    expect(readLang(undefined, [])).toBe("en");
  });

  it("is the one this device picked last, which it remembers", () => {
    const kept = store();
    keepLang(kept, "sv");
    expect(readLang(kept, ["en-US"])).toBe("sv");
    keepLang(kept, "en");
    expect(readLang(kept, ["sv-SE"])).toBe("en");
  });

  it("falls back to the browser's when storage refuses", () => {
    const refusing = {
      getItem: () => {
        throw new Error("SecurityError");
      },
      setItem: () => {
        throw new Error("SecurityError");
      },
    };
    expect(() => keepLang(refusing, "sv")).not.toThrow();
    expect(readLang(refusing, ["sv-SE"])).toBe("sv");
  });

  it("is listened for in the browser's own English, else US English, and in Sweden's Swedish", () => {
    expect(langTag("en", ["sv-SE", "en-GB"])).toBe("en-GB");
    expect(langTag("en", ["sv-SE"])).toBe("en-US");
    expect(langTag("sv", ["en-US"])).toBe("sv-SE");
  });
});

describe("the listening panel", () => {
  const d = (over: Partial<Dictation> = {}): Dictation => ({
    on: true,
    heard: { said: "Go with the udev rule.", guess: "the unplug case" },
    lang: "en",
    since: 0,
    begin: () => {},
    stop: () => {},
    send: () => {},
    cancel: () => {},
    pick: () => {},
    ...over,
  });

  it("shows the words heard, those still guessed apart, the language, and Cancel, Stop and Send", () => {
    const html = renderToStaticMarkup(<DictatePanel d={d()} to="Replying to the captain · GPS link recovery" sheet canSend />);
    expect(html).toContain('<dialog class="dict sheet" aria-label="Dictating, Replying to the captain · GPS link recovery">');
    expect(html).toContain(
      '<p class="dict-words"><span aria-live="polite">Go with the udev rule.</span> <span class="dict-guess">the unplug case</span>',
    );
    expect(html).toMatch(/role="radio" aria-checked="true"[^>]*>English<\/button><button[^>]*role="radio" aria-checked="false"[^>]*>Svenska<\/button>/);
    expect(html).toContain(">Cancel</button>");
    expect(html).toContain('aria-label="Stop, and edit the words"');
    expect(html).toMatch(/<button type="button" class="dict-send">Send/);
  });

  it("sits in the composer on a wider screen, and holds Send while the session takes none", () => {
    const html = renderToStaticMarkup(<DictatePanel d={d({ heard: { said: "", guess: "" } })} to="x" sheet={false} canSend={false} />);
    expect(html).toContain('<div class="dict" role="group" aria-label="Dictating">');
    expect(html).not.toContain("<dialog");
    expect(html).toContain("Speak, and your words show here.");
    expect(html).toMatch(/<button type="button" class="dict-send" disabled="">/);
  });

  it("counts how long it has listened", () => {
    expect(elapsed(0)).toBe("0:00");
    expect(elapsed(7_400)).toBe("0:07");
    expect(elapsed(83_000)).toBe("1:23");
  });
});
