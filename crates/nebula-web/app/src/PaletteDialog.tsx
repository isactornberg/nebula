import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import type { State } from "./api/contract";
import { age, composing, look } from "./model";
import { focusBack, marks, paletteGroups, paletteKey, queryWords, type PaletteCtx, type PaletteItem } from "./palette";
import { useKeyboardFit } from "./Detail";
import { Icon } from "./ui";
import "./palette.css";

/// ⌘K: one field that finds a session, an issue, a project or an action,
/// a modal dialog over the page, the whole screen on a phone. It takes the
/// focus while open and gives it back to what had it once it closes.
export function Palette({
  state,
  ctx,
  now,
  phone,
  onPick,
  onClose,
}: {
  state: State;
  ctx: PaletteCtx;
  now: number;
  phone: boolean;
  onPick: (item: PaletteItem) => void;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  const downOn = useRef<EventTarget | null>(null);
  const [query, setQuery] = useState("");
  /// The option lit, by its key, so a state push keeps it lit.
  const [lit, setLit] = useState<string | null>(null);
  const groups = paletteGroups(state, query, ctx);
  const items = groups.flatMap((g) => g.items);
  const found = items.findIndex((i) => i.key === lit);
  const active = items.length === 0 ? null : found >= 0 ? found : 0;
  const words = queryWords(query);
  useKeyboardFit(phone);

  // Layout effects, so the focus is back before whatever the pick does
  // runs, such as putting it in the reply box.
  useLayoutEffect(() => {
    const d = dialog.current!;
    const back = document.activeElement as HTMLElement | null;
    d.showModal();
    field.current?.focus();
    return () => {
      d.close();
      focusBack(back);
    };
  }, []);
  useEffect(() => {
    list.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }, [active, query]);

  const key = (e: KeyboardEvent<HTMLInputElement>) => {
    if (composing(e.nativeEvent)) return;
    const k = paletteKey(e.key, active, items.length);
    if (!k) return;
    e.preventDefault();
    if (k.kind === "move") setLit(k.to === null ? null : items[k.to]!.key);
    else if (k.kind === "run") onPick(items[k.at]!);
    else onClose();
  };
  // The page behind is inert while it is open; Tab goes round its own.
  const trap = (e: KeyboardEvent<HTMLDialogElement>) => {
    if (e.key !== "Tab") return;
    const all = [...e.currentTarget.querySelectorAll<HTMLElement>("input, button:not(:disabled)")];
    const at = all.indexOf(document.activeElement as HTMLElement);
    e.preventDefault();
    all[(at + (e.shiftKey ? -1 : 1) + all.length) % all.length]?.focus();
  };

  let n = 0;
  return (
    <dialog
      ref={dialog}
      className={phone ? "pal sheet" : "pal"}
      aria-label="Search or jump"
      onKeyDown={trap}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      // Chrome closes it on its own on an Esc it no longer lets be cancelled.
      // A close event from before it was shown again is no such close.
      onClose={(e) => !e.currentTarget.open && onClose()}
      // A press that began and ended outside the box closes it.
      onPointerDown={(e) => (downOn.current = e.target)}
      onClick={(e) => e.target === e.currentTarget && downOn.current === e.currentTarget && onClose()}
    >
      <div className="pal-box">
        <div className="pal-top">
          <label className="pal-find">
            <Icon name="search" size={18} />
            <input
            ref={field}
            className="pal-field"
            type="search"
            role="combobox"
            aria-label="Search sessions, issues, projects and actions"
            aria-expanded={items.length > 0}
            aria-controls="pal-list"
            aria-autocomplete="list"
            aria-activedescendant={active === null ? undefined : `pal-opt-${active}`}
            placeholder={phone ? "Search sessions and actions" : "Search sessions, issues and actions"}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setLit(null);
            }}
            onKeyDown={key}
            enterKeyHint="go"
            autoCapitalize="none"
            autoCorrect="off"
            autoComplete="off"
            spellCheck={false}
            />
          </label>
          {phone ? (
            <button type="button" className="pal-cancel" onClick={onClose}>
              Cancel
            </button>
          ) : (
            <kbd aria-hidden="true">esc</kbd>
          )}
        </div>
        <div className="pal-scroll">
          <div id="pal-list" role="listbox" aria-label="Results" className="pal-list" ref={list}>
            {groups.map((g) => (
              <div key={g.title} role="group" aria-labelledby={`pal-g-${g.title}`} className="pal-group">
                <div id={`pal-g-${g.title}`} className="pal-head">
                  {g.title}
                </div>
                {g.items.map((item) => {
                  const i = n++;
                  return (
                    <div
                      key={item.key}
                      id={`pal-opt-${i}`}
                      role="option"
                      aria-selected={i === active}
                      className="pal-opt"
                      data-kind={item.kind}
                      onPointerMove={() => i !== active && setLit(item.key)}
                      onMouseDown={(e) => e.preventDefault()}
                      onClick={() => onPick(item)}
                    >
                      <Option item={item} words={words} now={now} />
                      {i === active && !phone && (
                        <kbd aria-hidden="true" className="pal-enter">
                          ↵
                        </kbd>
                      )}
                    </div>
                  );
                })}
              </div>
            ))}
          </div>
          {items.length === 0 && (
            <p className="pal-none">
              No session, issue, project or action has {words.length > 1 && "every word of "}“{query.trim()}”.
            </p>
          )}
        </div>
        {!phone && (
          <div className="pal-foot">
            <span>
              <kbd aria-hidden="true">↑</kbd>
              <kbd aria-hidden="true">↓</kbd>
              move
            </span>
            <span>
              <kbd aria-hidden="true">↵</kbd>
              open
            </span>
            <span>
              <kbd aria-hidden="true">esc</kbd>
              close
            </span>
            <span className="pal-count" role="status">
              {items.length === 1 ? "1 result" : `${items.length} results`}
            </span>
          </div>
        )}
        {phone && (
          <span className="sr" role="status">
            {items.length === 1 ? "1 result" : `${items.length} results`}
          </span>
        )}
      </div>
    </dialog>
  );
}

function Marked({ text, words }: { text: string; words: string[] }) {
  return (
    <>
      {marks(text, words).map((p, i) => (p.hit ? <mark key={i}>{p.text}</mark> : p.text))}
    </>
  );
}

function Option({ item, words, now }: { item: PaletteItem; words: string[]; now: number }) {
  switch (item.kind) {
    case "session": {
      const s = item.session;
      const l = look(s);
      return (
        <>
          <span className="pal-ic">
            <span aria-hidden="true" className={`sdot ${l.tone}`} />
          </span>
          <span className="pal-text">
            <span className="pal-main">
              <Marked text={s.name} words={words} />
            </span>
            <span className="pal-meta">
              <Marked text={s.projectName} words={words} /> · {l.label}
            </span>
          </span>
          <span className="pal-side">{age(s.statusChangedAt, now)}</span>
        </>
      );
    }
    case "issue": {
      const i = item.issue;
      return (
        <>
          <span className="pal-ic">
            <Icon name="issues" size={15} className={i.stateType === "started" ? "pal-started" : undefined} />
          </span>
          <span className="pal-text">
            <span className="pal-main">
              <span className="m pal-key">
                <Marked text={i.key} words={words} />
              </span>
              <Marked text={i.title} words={words} />
            </span>
          </span>
          <span className="pal-side">{i.state}</span>
        </>
      );
    }
    case "project":
      return (
        <>
          <span className="pal-ic">
            <Icon name="folder" size={15} />
          </span>
          <span className="pal-text">
            <span className="pal-main">
              <Marked text={item.project.name} words={words} />
            </span>
          </span>
          <span className="pal-side">{item.sessions === 1 ? "1 session" : `${item.sessions} sessions`}</span>
        </>
      );
    case "action":
      return (
        <>
          <span className="pal-ic">
            <Icon name={item.icon} size={15} />
          </span>
          <span className="pal-text">
            <span className="pal-main">
              <Marked text={item.label} words={words} />
            </span>
          </span>
          {item.keys && (
            <span className="pal-side">
              <kbd aria-hidden="true">{item.keys}</kbd>
            </span>
          )}
        </>
      );
  }
}
