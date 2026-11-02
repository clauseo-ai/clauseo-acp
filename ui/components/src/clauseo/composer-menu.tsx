// The composer's `/` menu: the agent's commands, at the caret. It loads late, as the field is first focused
// (composer.tsx), and listens to the field itself: its keydown runs before the composer's own (a listener on the
// field before React's at the root), so Enter or Tab picks and never sends while the menu is open.
// - No query is read while an input method composes; a trigger typed inside a composition is read at
//   compositionend. Every insertion outside a composition reads it, a replacement too (Tamil99 and Anjal on macOS
//   edit by replacement and never compose).
// - A pick replaces the trigger's whole run through execCommand("insertText"), so one undo takes it back. A modified
//   Enter (Ctrl, Cmd, Alt) is no pick.
// - A list that changes while the menu is open is read again, the highlighted row kept by its name.
// - A polite live region says how many commands the menu opened with, then the highlighted one's name as the arrows
//   move: a textarea takes no combobox role, and VoiceOver follows aria-activedescendant reliably only on one.
// - It stands at the caret, placed again as the window resizes or anything scrolls while it is open.
import { useEffect, useId, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import { cn } from "../lib/cn.ts";
import { Mixed } from "../lib/mixed.tsx";
import { useThreadWords } from "../lib/words-thread.ts";
import { useLayer } from "../ui/layer-context.ts";
import { raised, row, rowNote } from "../ui/recipes-overlays.ts";
import type { SlashCommand } from "./composer.tsx";
import {
  caretOf,
  keepHighlight,
  matching,
  optionIds,
  picked,
  runEnd,
  placeMenu,
  slashAt,
  type Slash,
} from "./composer-menu-logic.ts";

/** What the field shows of the open menu: its list's id and the highlighted row's, for aria-controls and
 * aria-activedescendant; null while it's closed. */
export type MenuShown = { list: string; active: string } | null;

type Open = { slash: Slash; items: SlashCommand[]; hi: number };

export function CommandMenu({
  field,
  commands,
  composing,
  held,
  onShown,
  testIds,
}: {
  field: HTMLTextAreaElement;
  commands: readonly SlashCommand[];
  /** The composer's own record of a composition in progress. */
  composing: RefObject<boolean>;
  /** What the field held as it was first focused, or null when the commands came while it had the focus: words typed
   * since, while this chunk was on its way, are read as it arrives, so a `/` typed then opens the menu. Read once,
   * then emptied, so the menu mounting again never reads old words. */
  held: RefObject<string | null>;
  onShown: (shown: MenuShown) => void;
  testIds?: { menu?: string | undefined; item?: string | undefined } | undefined;
}) {
  const w = useThreadWords();
  const layer = useLayer();
  const list = useId();
  const [open, setOpenState] = useState<Open | null>(null);
  // the listeners read the latest of each through refs, so they are added once
  const now = useRef({ open, commands });
  now.current = { open, commands };
  // the trigger the lawyer closed with Escape: it stays closed until the caret leaves it
  const dismissed = useRef<number | null>(null);
  // a pick's own insertion must not open the menu again
  const picking = useRef(false);
  const plane = useRef<HTMLDivElement>(null);
  const shown = useRef<MenuShown>(null);
  // what the live region says: the count as the menu opens, then the highlighted row's name
  const [said, setSaid] = useState("");

  const setOpen = (next: Open | null) => {
    const was = now.current.open;
    now.current = { ...now.current, open: next };
    setOpenState(next);
    if (next === null) setSaid("");
    else if (was === null) {
      const n = next.items.length;
      setSaid(n === 1 ? w.th("1 command") : w.th("{n} commands", { n }));
    } else {
      const name = next.items[next.hi]?.name;
      if (name !== undefined && name !== was.items[was.hi]?.name) setSaid(`/${name}`);
    }
    const s: MenuShown =
      next === null ? null : { list, active: optionIds(list, next.items)[next.hi] ?? list };
    if (s?.active !== shown.current?.active || (s === null) !== (shown.current === null)) {
      shown.current = s;
      onShown(s);
    }
  };

  /** Reads the trigger at the caret again. Opens only when `canOpen`: after an insertion or a composition's end. */
  const refresh = (canOpen: boolean) => {
    if (composing.current) return;
    const cur = now.current.open;
    if (field.selectionStart !== field.selectionEnd) return setOpen(null);
    const slash = slashAt(field.value, field.selectionEnd);
    if (slash === null) {
      dismissed.current = null;
      return setOpen(null);
    }
    if (dismissed.current !== null && dismissed.current !== slash.at) dismissed.current = null;
    const items = matching(now.current.commands, slash.query);
    if (items.length === 0) return setOpen(null);
    if (cur !== null && cur.slash.at === slash.at) {
      return setOpen({ slash, items, hi: keepHighlight(items, cur.items[cur.hi]?.name ?? null) });
    }
    if (canOpen && dismissed.current !== slash.at) return setOpen({ slash, items, hi: 0 });
    setOpen(null);
  };
  const refreshRef = useRef(refresh);
  refreshRef.current = refresh;

  const pick = (command: SlashCommand) => {
    const cur = now.current.open;
    if (cur === null) return;
    const end = runEnd(field.value, field.selectionEnd);
    const text = picked(command, field.value[end]);
    setOpen(null);
    picking.current = true;
    try {
      field.focus();
      field.setSelectionRange(cur.slash.at, end);
      const doc = field.ownerDocument;
      const ok =
        typeof doc.execCommand === "function" && doc.execCommand("insertText", false, text);
      if (!ok || field.value.slice(cur.slash.at, cur.slash.at + text.length) !== text) {
        // no undo-keeping edit in this engine (happy-dom): put the words in, and tell the field's listeners
        field.setRangeText(text, cur.slash.at, end, "end");
        field.dispatchEvent(new Event("input", { bubbles: true }));
      }
    } finally {
      picking.current = false;
    }
  };
  const pickRef = useRef(pick);
  pickRef.current = pick;

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
        setOpen({ ...cur, hi: (cur.hi + (e.key === "ArrowDown" ? 1 : n - 1)) % n });
      } else if (
        (e.key === "Enter" || e.key === "Tab") &&
        !e.shiftKey &&
        !e.ctrlKey &&
        !e.metaKey &&
        !e.altKey
      ) {
        // never a send while the menu is open: the composer sees this Enter prevented and leaves it
        e.preventDefault();
        const item = cur.items[cur.hi];
        if (item !== undefined) pickRef.current(item);
      } else if (e.key === "Escape") {
        e.preventDefault();
        dismissed.current = cur.slash.at;
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

  // a `/` typed while this chunk was on its way: read as it arrives, once, and only for the chunk the focus asked for
  useEffect(() => {
    const was = held.current;
    held.current = null;
    if (was !== null && field.value !== was && field.ownerDocument.activeElement === field)
      refreshRef.current(true);
    // once, as the chunk arrives
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // the agent's list replaced while the menu is open: read again, the highlighted row kept by its name
  useEffect(() => {
    if (now.current.open !== null) refreshRef.current(false);
  }, [commands]);

  // closed, the field shows nothing of it
  useEffect(() => () => onShown(null), [onShown]);

  /** Puts the open menu at the caret. */
  const place = () => {
    const el = plane.current;
    if (el === null) return;
    const view = field.ownerDocument.defaultView;
    const at = placeMenu(
      caretOf(field),
      { width: el.offsetWidth, height: el.scrollHeight },
      { width: view?.innerWidth ?? 0, height: view?.innerHeight ?? 0 },
    );
    el.style.left = `${at.left}px`;
    el.style.top = `${at.top}px`;
    el.style.maxHeight = `${at.maxHeight}px`;
  };
  const placeRef = useRef(place);
  placeRef.current = place;

  // at the caret, once drawn, and the highlighted row in view
  useLayoutEffect(() => {
    const el = plane.current;
    if (el === null || open === null) return;
    place();
    const hi = el.children.item(open.hi);
    if (typeof hi?.scrollIntoView === "function") hi.scrollIntoView({ block: "nearest" });
  });

  // and again as the window resizes or anything scrolls under it, the field included; the list's own scroll aside
  const isOpen = open !== null;
  useEffect(() => {
    const view = field.ownerDocument.defaultView;
    if (!isOpen || view === null) return;
    const again = (e: Event) => {
      if (e.target !== plane.current) placeRef.current();
    };
    view.addEventListener("resize", again);
    view.addEventListener("scroll", again, { capture: true, passive: true });
    return () => {
      view.removeEventListener("resize", again);
      view.removeEventListener("scroll", again, { capture: true });
    };
  }, [isOpen, field]);

  const live = (
    <span className="sr-only" aria-live="polite" aria-atomic="true">
      {said}
    </span>
  );
  // one tree open or closed, so the live region is the same node throughout and is heard as it changes
  const ids = open === null ? [] : optionIds(list, open.items);
  return (
    <>
      {live}
      {open !== null &&
        createPortal(
          <div
            ref={plane}
            id={list}
            role="listbox"
            aria-label={w.th("Commands")}
            data-testid={testIds?.menu}
            className={cn(raised({ width: "menu" }), "fixed z-20")}
          >
            {open.items.map((c, i) => (
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
                  pick(c);
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
            ))}
          </div>,
          layer ?? field.ownerDocument.body,
        )}
    </>
  );
}
