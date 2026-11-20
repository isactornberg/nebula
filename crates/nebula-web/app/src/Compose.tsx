import { useRef, useState } from "react";
import { ImageStrip, imagesHold, pastedImages, useAttached } from "./Attach";
import { DictatePanel, KEYBOARD_MIC, useDictation } from "./Dictate";
import { sendsOnEnter } from "./model";
import { Icon } from "./ui";

export const IS_MAC = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

/// The modifier the page's shortcuts name: ⌘ on a Mac, Ctrl elsewhere.
export const MOD = IS_MAC ? "⌘" : "Ctrl ";

/// A touch screen, whose keyboard's Return adds a line.
const TOUCH = typeof matchMedia !== "undefined" && matchMedia("(pointer: coarse)").matches;

/// What is left in a box that held `box` once `sent` went: what was typed
/// after it while it was sending, or all of it where the box no longer
/// starts with it.
export function unsent(box: string, sent: string): string {
  const t = box.trimStart();
  return t.startsWith(sent) ? t.slice(sent.length).trimStart() : box;
}

/// A prompt being written: its text, its images, uploaded as they are
/// added, and dictation into it. `send` goes only with words or an image,
/// nothing holding it and nothing under way, and once `onSend` says the
/// prompt went the box lets go of what it sent; else it keeps what was
/// typed.
export function useCompose(
  onSend: (text: string, images: string[]) => Promise<boolean>,
  /// Why nothing can be sent now, if anything.
  blocked: string | null,
  busy: boolean,
  initial = "",
) {
  const [text, setText] = useState(initial);
  /// What dictation said went wrong, until the box changes.
  const [said, setSaid] = useState<string | null>(null);
  const { images, add, remove } = useAttached();
  const box = useRef<HTMLTextAreaElement>(null);
  const held = imagesHold(images);
  const ready = text.trim() !== "" || images.length > 0;
  const send = async (words: string) => {
    if ((!words && images.length === 0) || blocked || held || busy) return;
    const sent = images;
    if (await onSend(words, sent.flatMap((i) => (i.id ? [i.id] : [])))) {
      setText((t) => unsent(t, words));
      remove(...sent.map((i) => i.key));
    }
  };
  const dictation = useDictation(
    (e) => {
      const words = [text.trim(), e.words].filter(Boolean).join(" ");
      setSaid(e.problem);
      setText(words);
      if (e.send) void send(words);
      else requestAnimationFrame(() => toEnd(box.current));
    },
    () => {
      setSaid(KEYBOARD_MIC);
      box.current?.focus();
    },
  );
  const type = (t: string) => {
    setText(t);
    setSaid(null);
  };
  return { text, type, said, images, add, remove, box, held, ready, blocked, busy, send, dictation };
}

export type Compose = ReturnType<typeof useCompose>;

/// What a composer says: the box's label and placeholder, whom dictation
/// writes to, what its button does, and the mic's name.
export type ComposeWords = {
  label: string;
  placeholder: string;
  to: string;
  send: "Send" | "Start";
  dictate: string;
};

const UNDER_WAY = { Send: "Sending…", Start: "Starting…" };

/// The composer: a box as tall as its text, and under it its images,
/// Image, the mic and the button; on a phone a row of round buttons, the
/// mic the main one while the box is empty, and dictation in a sheet.
export function ComposeForm({ c, id, phone, words }: { c: Compose; id: string; phone: boolean; words: ComposeWords }) {
  const picker = useRef<HTMLInputElement>(null);
  const status = c.busy ? UNDER_WAY[words.send] : (c.said ?? c.blocked ?? c.held);
  const sendOff = !!c.blocked || !!c.held || c.busy || !c.ready;
  const field = (
    <>
      <label htmlFor={id} className="sr">
        {words.label}
      </label>
      <div className="compose-grow" data-text={`${c.text} `}>
        <textarea
          id={id}
          ref={c.box}
          rows={phone ? 1 : 2}
          value={c.text}
          onChange={(e) => c.type(e.target.value)}
          onKeyDown={(e) => {
            if (!sendsOnEnter(e.nativeEvent, TOUCH)) return;
            e.preventDefault();
            void c.send(c.text.trim());
          }}
          onPaste={(e) => {
            const pasted = pastedImages(e.clipboardData.files);
            if (pasted.length === 0) return;
            e.preventDefault();
            c.add(pasted);
          }}
          placeholder={words.placeholder}
          enterKeyHint="enter"
        />
      </div>
    </>
  );
  const fileInput = (
    <input
      ref={picker}
      type="file"
      accept="image/*"
      multiple
      hidden
      onChange={(e) => {
        c.add([...(e.target.files ?? [])]);
        e.target.value = "";
      }}
    />
  );
  const panel = c.dictation.on && (
    <DictatePanel d={c.dictation} to={words.to} sheet={phone} canSend={!c.blocked && !c.held && !c.busy} />
  );
  return (
    <form
      className="compose"
      onSubmit={(e) => {
        e.preventDefault();
        void c.send(c.text.trim());
      }}
      onDragOver={(e) => e.dataTransfer.types.includes("Files") && e.preventDefault()}
      onDrop={(e) => {
        const dropped = pastedImages(e.dataTransfer.files);
        if (dropped.length === 0) return;
        e.preventDefault();
        c.add(dropped);
      }}
    >
      {phone ? (
        <>
          <ImageStrip named images={c.images} onRemove={c.remove} />
          <div className="compose-row">
            <button type="button" className="round-btn" aria-label="Add an image" onClick={() => picker.current?.click()}>
              <Icon name="image" size={20} />
            </button>
            {fileInput}
            {field}
            {c.ready ? (
              <span className="compose-side">
                <button type="button" className="round-btn small" aria-label="Dictate more" onClick={c.dictation.begin}>
                  <Icon name="mic" size={18} />
                </button>
                <button type="submit" className="round-btn send" aria-label={words.send} disabled={sendOff}>
                  <Icon name="arrowUp" size={20} />
                </button>
              </span>
            ) : (
              <button type="button" className="round-btn mic" aria-label={words.dictate} onClick={c.dictation.begin}>
                <Icon name="mic" size={21} />
              </button>
            )}
          </div>
          {status && (
            <p className="compose-line" role="status">
              {status}
            </p>
          )}
          {panel}
        </>
      ) : c.dictation.on ? (
        panel
      ) : (
        <>
          {field}
          <div className="compose-foot">
            <ImageStrip named images={c.images} onRemove={c.remove} />
            <button type="button" className="compose-btn" onClick={() => picker.current?.click()}>
              <Icon name="image" size={15} />
              Image
            </button>
            {fileInput}
            <span className={status ? "compose-line" : "compose-line resting"} role="status">
              {status ?? "Paste or drop an image"}
            </span>
            <button type="button" className="compose-mic" aria-label={words.dictate} title={words.dictate} onClick={c.dictation.begin}>
              <Icon name="mic" size={17} />
            </button>
            <button type="submit" className="send" disabled={sendOff}>
              {words.send}
              <kbd aria-hidden="true">{TOUCH ? `${MOD}↩` : "↩"}</kbd>
            </button>
          </div>
        </>
      )}
    </form>
  );
}

/// Focuses `box` with the caret after its text, where dictation left it.
function toEnd(box: HTMLTextAreaElement | null) {
  if (!box) return;
  box.focus();
  box.setSelectionRange(box.value.length, box.value.length);
}
