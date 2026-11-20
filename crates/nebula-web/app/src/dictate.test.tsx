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

function listen(tag = "en-US") {
  const heard: Heard[] = [];
  const ended: (Ended | null)[] = [];
  const l = startListening(FakeRecognizer, false, tag, { heard: (h) => heard.push(h), ended: (e) => ended.push(e) });
  return { l: l!, r: FakeRecognizer.made.at(-1)!, heard, ended };
}

afterEach(() => {
  FakeRecognizer.made = [];
  FakeRecognizer.failsToStart = false;
  vi.useRealTimers();
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
