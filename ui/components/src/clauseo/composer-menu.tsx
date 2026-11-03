// The composer's menus at the caret: `/` for the agent's commands, `@` for the matter's papers. It loads late, as the
// field is first focused (composer.tsx), and listens to the field itself: its keydown runs before the composer's own (a
// listener on the field before React's at the root), so Enter or Tab picks and never sends while a menu is open.
// - No query is read while an input method composes; a trigger typed inside a composition is read at
//   compositionend. Every insertion outside a composition reads it, a replacement too (Tamil99 and Anjal on macOS
//   edit by replacement and never compose).
// - A pick replaces the trigger's whole run through execCommand("insertText"), so one undo takes it back. A modified
//   Enter (Ctrl, Cmd, Alt) is no pick.
// - A list that changes while the menu is open is read again, the highlighted row kept by its name (a command) or its
//   id (a paper).
// - A polite live region says how many rows the menu opened with (or that the papers are loading, then how many
//   there are), then the highlighted one's name as the arrows move: a textarea takes no combobox role, and VoiceOver
//   follows aria-activedescendant reliably only on one.
// - It stands at the caret's x, 4 px clear of the field, inside what clips it (the room's column), its rows scrolling
//   under about eight; placed again as the window resizes, the field's size changes or anything scrolls while open.
// - A pick is a token (composer-tokens.ts), held and drawn from this chunk as long as the menu lives; a trigger that
//   starts a token is no trigger.
// - `@`: the room's `load` is called each time the menu opens (the room caches); the last list answered stands in the
//   meantime, or "Loading papers…" before the first. A query that matches nothing closes the menu, as `/` does; a
//   matter with no papers, a list that couldn't be read, more matches than the 50 rows shown and a list that stops
//   short each get a line under the rows. The papers picked here are remembered by their token's text, so a token
//   undo brings back mentions its paper again.
import { useEffect, useId, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import { cn } from "../lib/cn.ts";
import { Mixed } from "../lib/mixed.tsx";
import { useThreadWords } from "../lib/words-thread.ts";
import { useLayer } from "../ui/layer-context.ts";
import { raised, row, rowNote } from "../ui/recipes-overlays.ts";
import type { MentionItem, Mentions, SlashCommand } from "./composer.tsx";
import {
  caretOf,
  clipOf,
  keepHighlight,
  keyIds,
  matching,
  runEnd,
  placeMenu,
  triggerAt,
  type Trigger,
} from "./composer-menu-logic.ts";
import {
  filterMentions,
  foldMentions,
  keepMention,
  MAX_MENTION_ROWS,
  mentionedIn,
  mentionText,
  type Folded,
} from "./composer-mention-logic.ts";
import { holdTokens, type Tokens } from "./composer-tokens.ts";

/** What the field shows of the open menu: its list's id and the highlighted row's, for aria-controls and
 * aria-activedescendant, each undefined while the menu has no rows (the papers loading); null while it's closed. */
export type MenuShown = { list: string | undefined; active: string | undefined } | null;

/** What stands under the `@` menu's rows, if anything. */
type Note = "loading" | "failed" | "none" | null;

type Open =
  | { kind: "command"; trigger: Trigger; items: SlashCommand[]; hi: number }
  | {
      kind: "mention";
      trigger: Trigger;
      items: MentionItem[];
      /** How many matched, the rows past MAX_MENTION_ROWS included. */
      total: number;
      hi: number;
      note: Note;
      partial: boolean;
    };

/** The papers as this chunk last had them. */
type Loaded = { list: Folded[]; partial: boolean } | "loading" | "failed" | null;

/** The keys of the open menu's rows, for their element ids: a command's name, a paper's id. */
const keysOf = (o: Open): string[] =>
  o.kind === "command" ? o.items.map((c) => c.name) : o.items.map((m) => m.id);

/** The text a row's pick puts in the field, without the space after it. */
const tokenOf = (o: Open, i: number): string | undefined => {
  if (o.kind === "command") {
    const c = o.items[i];
    return c === undefined ? undefined : `/${c.name}`;
  }
  const m = o.items[i];
  return m === undefined ? undefined : mentionText(m.text);
};

/** What the live region says of a row as it is lit: a command with its slash, a paper by its name. */
const sayOf = (o: Open, i: number): string | undefined =>
  o.kind === "command" ? tokenOf(o, i) : o.items[i]?.label;

export function ComposerMenu({
  field,
  commands,
  mentions,
  composing,
  held,
  onShown,
  sync,
  picks,
  testIds,
  frame,
}: {
  field: HTMLTextAreaElement;
  commands: readonly SlashCommand[];
  /** The room's papers for `@`; without them `@` is a character. */
  mentions?: Mentions | undefined;
  /** The composer's own record of a composition in progress. */
  composing: RefObject<boolean>;
  /** What the field held as it was first focused, or null when the commands came while it had the focus: words typed
   * since, while this chunk was on its way, are read as it arrives, so a `/` typed then opens the menu. Read once,
   * then emptied, so the menu mounting again never reads old words. */
  held: RefObject<string | null>;
  onShown: (shown: MenuShown) => void;
  /** Where the composer finds how to read the field again after it set the words in place, with no input event. */
  sync?: { current: (() => void) | null } | undefined;
  /** Where the composer's handle finds the papers the field's tokens mention now. */
  picks?: { current: (() => readonly MentionItem[]) | null } | undefined;
  testIds?: { menu?: string | undefined; item?: string | undefined } | undefined;
  /** The field's drawn box, which the menu stands clear of; the field itself without it. */
  frame?: RefObject<HTMLElement | null> | undefined;
}) {
  const w = useThreadWords();
  const layer = useLayer();
  const list = useId();
  const [open, setOpenState] = useState<Open | null>(null);
  // the listeners read the latest of each through refs, so they are added once
  const now = useRef({ open, commands, mentions });
  now.current = { open, commands, mentions };
  // the trigger the lawyer closed with Escape: it stays closed until the caret leaves it
  const dismissed = useRef<number | null>(null);
  // a pick's own insertion must not open the menu again
  const picking = useRef(false);
  const plane = useRef<HTMLDivElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const shown = useRef<MenuShown>(null);
  // what the live region says: the count as the menu opens, then the highlighted row's name
  const [said, setSaid] = useState("");
  // the picks, as tokens, and the overlay that draws them behind the field
  const cover = useRef<HTMLDivElement>(null);
  const tokens = useRef<Tokens | null>(null);
  // the papers, as last loaded, and which load is the latest
  const loaded = useRef<Loaded>(null);
  const loads = useRef(0);
  // the papers picked in this field, by their token's text: undo brings a token back, and it mentions its paper again
  const known = useRef(new Map<string, MentionItem>());

  /** Every token text the field can hold: the agent's commands, and the papers picked here. */
  const texts = () => [...now.current.commands.map((c) => `/${c.name}`), ...known.current.keys()];

  const countWords = (o: Open): string => {
    if (o.kind === "command") {
      const n = o.items.length;
      return n === 1 ? w.th("1 command") : w.th("{n} commands", { n });
    }
    if (o.note === "loading") return w.th("Loading papers…");
    if (o.note === "failed") return w.th("Couldn't read this matter's papers.");
    if (o.note === "none") return w.t("No papers yet.");
    return o.total === 1 ? w.th("1 paper") : w.th("{n} papers", { n: o.total });
  };

  const setOpen = (next: Open | null) => {
    const was = now.current.open;
    now.current = { ...now.current, open: next };
    setOpenState(next);
    if (next === null) setSaid("");
    else if (
      was === null ||
      was.kind !== next.kind ||
      (next.kind === "mention" && was.kind === "mention" && was.note !== next.note)
    ) {
      setSaid(countWords(next));
    } else {
      const name = sayOf(next, next.hi);
      if (name !== undefined && name !== sayOf(was, was.hi)) setSaid(name);
    }
    const ids = next === null ? [] : keyIds(list, keysOf(next));
    const s: MenuShown =
      next === null
        ? null
        : next.items.length === 0
          ? { list: undefined, active: undefined }
          : { list, active: ids[next.hi] ?? list };
    if (
      s?.active !== shown.current?.active ||
      s?.list !== shown.current?.list ||
      (s === null) !== (shown.current === null)
    ) {
      shown.current = s;
      onShown(s);
    }
  };

  /** The `@` menu's state for the trigger, from the papers as loaded; null when a query matches none of them. */
  const mentionOpen = (trigger: Trigger, cur: Open | null): Open | null => {
    const l = loaded.current;
    const base = { kind: "mention" as const, trigger, hi: 0, total: 0, partial: false };
    if (l === null || l === "loading") return { ...base, items: [], note: "loading" };
    if (l === "failed") return { ...base, items: [], note: "failed" };
    if (l.list.length === 0) return { ...base, items: [], note: "none", partial: l.partial };
    const { items, total } = filterMentions(l.list, trigger.query);
    if (total === 0) return null;
    const lit = cur?.kind === "mention" ? (cur.items[cur.hi]?.id ?? null) : null;
    return { ...base, items, total, hi: keepMention(items, lit), note: null, partial: l.partial };
  };

  /** Asks the room for the papers, as `@` opens the menu: the last list stands until the answer. */
  const load = () => {
    const source = now.current.mentions;
    if (source === undefined) return;
    const seq = ++loads.current;
    if (loaded.current === null || loaded.current === "failed") loaded.current = "loading";
    let answer: Promise<{ items: readonly MentionItem[]; partial?: boolean | undefined }>;
    try {
      answer = source.load();
    } catch (e) {
      answer = Promise.reject(e);
    }
    answer.then(
      (got) => {
        if (seq !== loads.current) return;
        loaded.current = { list: foldMentions(got.items), partial: got.partial === true };
        if (now.current.open?.kind === "mention") refreshRef.current(false);
      },
      () => {
        if (seq !== loads.current) return;
        if (typeof loaded.current !== "object" || loaded.current === null)
          loaded.current = "failed";
        if (now.current.open?.kind === "mention") refreshRef.current(false);
      },
    );
  };

  /** Reads the trigger at the caret again. Opens only when `canOpen`: after an insertion or a composition's end. */
  const refresh = (canOpen: boolean) => {
    if (composing.current) return;
    const cur = now.current.open;
    if (field.selectionStart !== field.selectionEnd) return setOpen(null);
    const chars =
      (now.current.commands.length > 0 ? "/" : "") +
      (now.current.mentions !== undefined ? "@" : "");
    const found = triggerAt(field.value, field.selectionEnd, chars);
    // a picked command's own slash, or a picked paper's @, opens nothing
    const trigger =
      found !== null && tokens.current?.held().some((t) => t.start === found.at) === true
        ? null
        : found;
    if (trigger === null) {
      dismissed.current = null;
      return setOpen(null);
    }
    if (dismissed.current !== null && dismissed.current !== trigger.at) dismissed.current = null;
    const same = cur !== null && cur.trigger.at === trigger.at && cur.trigger.char === trigger.char;
    if (!same && !(canOpen && dismissed.current !== trigger.at)) return setOpen(null);
    if (trigger.char === "/") {
      const items = matching(now.current.commands, trigger.query);
      if (items.length === 0) return setOpen(null);
      const lit = same && cur.kind === "command" ? (cur.items[cur.hi]?.name ?? null) : null;
      return setOpen({ kind: "command", trigger, items, hi: keepHighlight(items, lit) });
    }
    // a new `@` asks the room again; its cache answers at once, or after 30 s from the host
    if (!same) load();
    setOpen(mentionOpen(trigger, same ? cur : null));
  };
  const refreshRef = useRef(refresh);
  refreshRef.current = refresh;

  const pick = (i: number) => {
    const cur = now.current.open;
    if (cur === null) return;
    const token = tokenOf(cur, i);
    if (token === undefined) return;
    if (cur.kind === "mention") {
      const m = cur.items[i];
      if (m !== undefined) known.current.set(token, m);
      tokens.current?.texts(texts());
    }
    const end = runEnd(field.value, field.selectionEnd);
    const following = field.value[end];
    const text = following === undefined || !/\s/u.test(following) ? `${token} ` : token;
    setOpen(null);
    picking.current = true;
    tokens.current?.expect(cur.trigger.at, token);
    try {
      field.focus();
      field.setSelectionRange(cur.trigger.at, end);
      const doc = field.ownerDocument;
      const ok =
        typeof doc.execCommand === "function" && doc.execCommand("insertText", false, text);
      if (!ok || field.value.slice(cur.trigger.at, cur.trigger.at + text.length) !== text) {
        // no undo-keeping edit in this engine (happy-dom): put the words in, and tell the field's listeners
        field.setRangeText(text, cur.trigger.at, end, "end");
        field.dispatchEvent(new Event("input", { bubbles: true }));
      }
    } finally {
      picking.current = false;
    }
  };
  const pickRef = useRef(pick);
  pickRef.current = pick;

  // the tokens, held from here, their listeners on the field before the menu's, so the menu reads them moved; the
  // composer's in-place edits read through `sync`, and its handle's `mentioned` through `picks`
  useEffect(() => {
    const el = cover.current;
    if (el === null) return;
    const t = holdTokens(field, el, composing, texts());
    tokens.current = t;
    if (sync !== undefined) sync.current = t.sync;
    const mentioned = () => mentionedIn(t.now(), known.current);
    if (picks !== undefined) picks.current = mentioned;
    return () => {
      t.stop();
      tokens.current = null;
      if (sync !== undefined && sync.current === t.sync) sync.current = null;
      if (picks !== undefined && picks.current === mentioned) picks.current = null;
    };
    // the token texts are read through refs as the tokens start
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [field, composing, sync, picks]);

  useEffect(() => {
    const onInput = (e: Event) => {
      if (picking.current || composing.current || (e as InputEvent).isComposing === true) return;
      const kind = (e as InputEvent).inputType ?? "insertText";
      refreshRef.current(kind.startsWith("insert"));
    };
    const onEnd = () => {
      // the composition's words, and a trigger typed among them, once the engine has settled the value
      setTimeout(() => {
        if (field.ownerDocument.activeElement === field) refreshRef.current(true);
      }, 0);
    };
    const onKey = (e: KeyboardEvent) => {
      const cur = now.current.open;
      if (cur === null || composing.current || e.isComposing || e.keyCode === 229) return;
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const n = cur.items.length;
        if (n === 0) return;
        setOpen({ ...cur, hi: (cur.hi + (e.key === "ArrowDown" ? 1 : n - 1)) % n });
      } else if (
        (e.key === "Enter" || e.key === "Tab") &&
        !e.shiftKey &&
        !e.ctrlKey &&
        !e.metaKey &&
        !e.altKey
      ) {
        // never a send while the menu is open, rows or not (the papers loading): the composer sees this Enter
        // prevented and leaves it
        e.preventDefault();
        if (cur.items[cur.hi] !== undefined) pickRef.current(cur.hi);
      } else if (e.key === "Escape") {
        e.preventDefault();
        dismissed.current = cur.trigger.at;
        setOpen(null);
      }
    };
    const onBlur = () => {
      if (now.current.open !== null) setOpen(null);
    };
    const onSelection = () => {
      if (now.current.open !== null && field.ownerDocument.activeElement === field)
        refreshRef.current(false);
    };
    const doc = field.ownerDocument;
    field.addEventListener("input", onInput);
    field.addEventListener("compositionend", onEnd);
    field.addEventListener("keydown", onKey);
    field.addEventListener("blur", onBlur);
    doc.addEventListener("selectionchange", onSelection);
    return () => {
      field.removeEventListener("input", onInput);
      field.removeEventListener("compositionend", onEnd);
      field.removeEventListener("keydown", onKey);
      field.removeEventListener("blur", onBlur);
      doc.removeEventListener("selectionchange", onSelection);
    };
    // the listeners read the latest state through refs; the field and the composition record stay
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [field, composing]);

  // a trigger typed while this chunk was on its way: read as it arrives, once, and only for the chunk the focus asked
  // for
  useEffect(() => {
    const was = held.current;
    held.current = null;
    if (was !== null && field.value !== was && field.ownerDocument.activeElement === field)
      refreshRef.current(true);
    // once, as the chunk arrives
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // the agent's list replaced: a token whose command has gone is plain text; and while the menu is open, read again,
  // the highlighted row kept by its name
  useEffect(() => {
    tokens.current?.texts(texts());
    if (now.current.open !== null) refreshRef.current(false);
    // the papers picked here are read through their ref
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [commands]);

  // another room's papers (another matter): the loaded list is theirs no longer
  const firstMentions = useRef(mentions);
  useEffect(() => {
    if (firstMentions.current === mentions) return;
    firstMentions.current = mentions;
    loads.current += 1;
    loaded.current = null;
    if (now.current.open?.kind === "mention") refreshRef.current(false);
  }, [mentions]);

  // closed, the field shows nothing of it
  useEffect(() => () => onShown(null), [onShown]);

  /** Puts the open menu at the caret's x, clear of the field above or below it, inside what clips it. Its height is
   * read with its last bound let go: the rows up to their own cap, and the lines under them. */
  const place = () => {
    const el = plane.current;
    if (el === null) return;
    const box = (frame?.current ?? field).getBoundingClientRect();
    el.style.maxHeight = "";
    const at = placeMenu(
      { x: caretOf(field).x, top: box.top, bottom: box.bottom },
      { width: el.offsetWidth, height: el.offsetHeight },
      clipOf(el),
    );
    el.style.left = `${at.left}px`;
    el.style.top = `${at.top}px`;
    el.style.maxHeight = `${at.maxHeight}px`;
  };
  const placeRef = useRef(place);
  placeRef.current = place;

  // at the caret, once drawn, and the highlighted row in view: the rows scroll by themselves, never the room's
  // column under them, which scrollIntoView would move too
  useLayoutEffect(() => {
    if (plane.current === null || open === null) return;
    place();
    const rows = box.current;
    const hi = rows?.children.item(open.hi);
    if (rows === null || rows === undefined || hi === null || hi === undefined) return;
    const r = hi.getBoundingClientRect();
    const b = rows.getBoundingClientRect();
    if (r.top < b.top) rows.scrollTop -= b.top - r.top;
    else if (r.bottom > b.bottom) rows.scrollTop += Math.min(r.bottom - b.bottom, r.top - b.top);
  });

  // and again as the window resizes or anything scrolls under it, the field included; the rows' own scroll aside.
  // And as the field's own size changes: a narrower window rewraps the words and the field grows a line, but the
  // window's resize comes before that grow (the composer fits the field from its own size observer), so a menu placed
  // on the resize alone stood over the line's start (WebView2, M2 at 100 px narrower). Placed once per frame, after it.
  const isOpen = open !== null;
  useEffect(() => {
    const view = field.ownerDocument.defaultView;
    if (!isOpen || view === null) return;
    const again = (e: Event) => {
      if (e.target !== plane.current && e.target !== box.current) placeRef.current();
    };
    view.addEventListener("resize", again);
    view.addEventListener("scroll", again, { capture: true, passive: true });
    let frame: number | null = null;
    const grown =
      typeof ResizeObserver === "function"
        ? new ResizeObserver(() => {
            if (frame !== null) return;
            frame = view.requestAnimationFrame(() => {
              frame = null;
              placeRef.current();
            });
          })
        : null;
    grown?.observe(field);
    return () => {
      view.removeEventListener("resize", again);
      view.removeEventListener("scroll", again, { capture: true });
      grown?.disconnect();
      if (frame !== null) view.cancelAnimationFrame(frame);
    };
  }, [isOpen, field]);

  const live = (
    <span className="sr-only" aria-live="polite" aria-atomic="true">
      {said}
    </span>
  );
  // the tokens' boxes, behind the field's words; holdTokens fills it, React never does
  const overlay = <div ref={cover} className="room-field-tokens" aria-hidden="true" />;
  // the lines under the `@` menu's rows
  const notes: string[] = [];
  if (open?.kind === "mention") {
    if (open.note !== null) notes.push(countWords(open));
    if (open.total > open.items.length)
      notes.push(
        w.th("First {shown} of {n}. Type to narrow.", { shown: MAX_MENTION_ROWS, n: open.total }),
      );
    if (open.partial) notes.push(w.th("Not every file in this folder is listed."));
  }
  // one tree open or closed, so the live region is the same node throughout and is heard as it changes
  const ids = open === null ? [] : keyIds(list, keysOf(open));
  return (
    <>
      {live}
      {overlay}
      {open !== null &&
        createPortal(
          // the rows scroll inside, up to about eight; the lines under them (the count past 50, a list that stops
          // short) stay in view below them
          <div
            ref={plane}
            className={cn(raised({ width: "menu" }), "fixed z-20 flex flex-col overflow-hidden")}
          >
            {open.items.length > 0 && (
              <div
                ref={box}
                id={list}
                className="min-h-0 max-h-menu-list overflow-y-auto"
                role="listbox"
                aria-label={open.kind === "command" ? w.th("Commands") : w.th("Papers")}
                data-testid={testIds?.menu}
              >
                {open.kind === "command"
                  ? open.items.map((c, i) => (
                      <div
                        key={ids[i]}
                        id={ids[i]}
                        role="option"
                        aria-selected={i === open.hi}
                        data-highlighted={i === open.hi ? "" : undefined}
                        data-testid={testIds?.item}
                        className={cn(row, "items-start")}
                        // the field keeps the focus: a press picks, as Enter does
                        onPointerDown={(e) => {
                          e.preventDefault();
                          pick(i);
                        }}
                        onPointerMove={() => {
                          if (i !== open.hi) setOpen({ ...open, hi: i });
                        }}
                      >
                        <span className="min-w-0">
                          <span className="block">
                            <bdi>/{c.name}</bdi>
                            {c.input_hint !== undefined && c.input_hint !== "" && (
                              <span className="text-muted">
                                {" "}
                                <Mixed>{c.input_hint}</Mixed>
                              </span>
                            )}
                          </span>
                          {c.description !== "" && (
                            <span className={rowNote}>
                              <Mixed>{c.description}</Mixed>
                            </span>
                          )}
                        </span>
                      </div>
                    ))
                  : open.items.map((m, i) => (
                      <div
                        key={ids[i]}
                        id={ids[i]}
                        role="option"
                        aria-selected={i === open.hi}
                        data-highlighted={i === open.hi ? "" : undefined}
                        data-testid={testIds?.item}
                        className={cn(row, "items-start")}
                        onPointerDown={(e) => {
                          e.preventDefault();
                          pick(i);
                        }}
                        onPointerMove={() => {
                          if (i !== open.hi) setOpen({ ...open, hi: i });
                        }}
                      >
                        <span className="min-w-0">
                          <span className="block truncate">
                            <bdi>{m.label}</bdi>
                          </span>
                          {m.detail !== undefined && m.detail !== "" && (
                            <span className={cn(rowNote, "truncate")}>
                              <bdi>{m.detail}</bdi>
                            </span>
                          )}
                        </span>
                      </div>
                    ))}
              </div>
            )}
            {notes.map((n) => (
              <p key={n} className={cn(rowNote, "shrink-0 px-s3 py-s1-5")}>
                {n}
              </p>
            ))}
          </div>,
          // inside the field's landmark when no layer is given (the room's main has none), so the open menu is
          // content of the page's regions (axe's region rule); fixed, it stands at the caret wherever it mounts, bounded by
          // what clips it there (clipOf)
          layer ?? field.closest("main, aside") ?? field.ownerDocument.body,
        )}
    </>
  );
}
