// The composer's `/` menu: the agent's commands, at the caret. It loads late, as the field is first focused
// (composer.tsx), and listens to the field itself: its keydown runs before the composer's own (a listener on the
// field before React's at the root), so Enter or Tab picks and never sends while the menu is open.
// - No query is read while an input method composes; a trigger typed inside a composition is read at
//   compositionend. Every insertion outside a composition reads it, a replacement too (Tamil99 and Anjal on macOS
//   edit by replacement and never compose).
// - A pick replaces the trigger through execCommand("insertText"), so one undo takes it back.
// - A list that changes while the menu is open is read again, the highlighted row kept by its name.
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
  picked,
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
  /** What the field held as it was first focused: words typed since, while this chunk was on its way, are read as
   * it arrives, so a `/` typed then opens the menu. */
  held: string;
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

  const setOpen = (next: Open | null) => {
    now.current = { ...now.current, open: next };
    setOpenState(next);
    const s: MenuShown = next === null ? null : { list, active: `${list}-${next.hi}` };
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
    const text = picked(command, field.value[field.selectionEnd]);
    setOpen(null);
    picking.current = true;
    try {
      field.focus();
      field.setSelectionRange(cur.slash.at, field.selectionEnd);
      const doc = field.ownerDocument;
      const ok =
        typeof doc.execCommand === "function" && doc.execCommand("insertText", false, text);
      if (!ok || field.value.slice(cur.slash.at, cur.slash.at + text.length) !== text) {
        // no undo-keeping edit in this engine (happy-dom): put the words in, and tell the field's listeners
        field.setRangeText(text, cur.slash.at, field.selectionEnd, "end");
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
      } else if ((e.key === "Enter" && !e.shiftKey) || (e.key === "Tab" && !e.shiftKey)) {
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

  // a `/` typed while this chunk was on its way: read as it arrives
  useEffect(() => {
    if (field.value !== held && field.ownerDocument.activeElement === field)
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

  // at the caret, once drawn, and the highlighted row in view
  useLayoutEffect(() => {
    const el = plane.current;
    if (el === null || open === null) return;
    const view = field.ownerDocument.defaultView;
    const place = placeMenu(
      caretOf(field),
      { width: el.offsetWidth, height: el.scrollHeight },
      { width: view?.innerWidth ?? 0, height: view?.innerHeight ?? 0 },
    );
    el.style.left = `${place.left}px`;
    el.style.top = `${place.top}px`;
    el.style.maxHeight = `${place.maxHeight}px`;
    const hi = el.children.item(open.hi);
    if (typeof hi?.scrollIntoView === "function") hi.scrollIntoView({ block: "nearest" });
  });

  if (open === null) return null;
  return createPortal(
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
          key={`${i}:${c.name}`}
          id={`${list}-${i}`}
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
  );
}
