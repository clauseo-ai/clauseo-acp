// The composer's tokens on the page: the ranges composer-tokens-logic.ts keeps, held to the field as it changes, and
// the overlay that draws a box behind each. It loads with the `/` menu's chunk, which makes the tokens (a pick), and
// lives as long as the menu does. Nothing here renders through React: a keystroke changes the overlay in place.
// - Backspace right after a token, or Delete right before one, selects it and deletes it through the engine
//   (execCommand("delete")), so one undo brings it back. Left at a token's end goes to its start, Right at its start
//   to its end; a caret put inside one any other way (a press, Up or Down) goes to its nearer edge, and a selection
//   grows to whole tokens. Never while an input method composes, and never with Ctrl, Cmd or Alt held.
// - Undo and redo read the value again for the tokens' texts (the agent's commands, the papers picked here), so a
//   token they bring back is one again.
// - The overlay is a copy of the field behind it: the field's face, padding, line height and transform (the
//   Devanagari nudge) copied, its client width (a scrollbar can't move the wrapping), its scroll followed. It holds the
//   value as one text node, drawn transparent, so its words shape as one run as the field's do (a box around a token
//   would split the run, and kerning across the split moved the words after it: the spike's M3), and draws only the
//   boxes, each placed from a Range over the token's characters. Built with text nodes and createElement only.
import { FACE } from "./composer-menu-logic.ts";
import {
  scanTokens,
  shiftTokens,
  snapped,
  standing,
  stillListed,
  tokenEndingAt,
  tokenStartingAt,
  withPick,
  wordDelete,
  type Token,
} from "./composer-tokens-logic.ts";

export type Tokens = {
  /** The tokens that count now, in order: those standing alone (one typed against is plain text while it is). */
  now: () => readonly Token[];
  /** Every token held, typed against or not: a trigger at one's start is still no trigger. */
  held: () => readonly Token[];
  /** A pick about to put `text` in at `start`: the edit that does it makes it a token. */
  expect: (start: number, text: string) => void;
  /** The tokens' texts changed (`/review` for each of the agent's commands, `@Notice.pdf` for each paper picked): a
   * token whose text has gone is plain text, and undo finds the new ones. */
  texts: (texts: readonly string[]) => void;
  /** The field changed with no input event of its own (the composer set its value in place): read it again. */
  sync: () => void;
  /** Takes the listeners off and empties the overlay. */
  stop: () => void;
};

const px = (v: string) => {
  const n = parseFloat(v);
  return Number.isFinite(n) ? n : 0;
};

/** Holds the field's tokens and draws them on `overlay`, an element beside the field in its positioned box. */
export function holdTokens(
  field: HTMLTextAreaElement,
  overlay: HTMLElement,
  composing: { readonly current: boolean },
  texts: readonly string[],
): Tokens {
  const doc = field.ownerDocument;
  const view = doc.defaultView;
  let listed = texts;
  let value = field.value;
  // what the field held as the menu's chunk arrived: a command standing alone is a token, as after an undo
  let tokens: Token[] = scanTokens(value, listed, []);
  let pending: Token | null = null;
  const text = doc.createTextNode("");

  /** The overlay laid over the field, the field's words in it, and a box behind each token. */
  const draw = () => {
    const shown = standing(value, tokens);
    if (shown.length === 0) {
      if (overlay.firstChild !== null) overlay.replaceChildren();
      return;
    }
    const style = view?.getComputedStyle(field);
    if (style !== undefined) {
      for (const p of FACE) overlay.style[p] = style[p];
      overlay.style.transform = style.transform;
    }
    // as a textarea keeps them: line breaks and runs of spaces stand, so a box after Shift+Enter or two spaces is
    // drawn where the words are (the stylesheet says so too)
    overlay.style.whiteSpace = "pre-wrap";
    const bx = px(style?.borderLeftWidth ?? "") + px(style?.borderRightWidth ?? "");
    const by = px(style?.borderTopWidth ?? "") + px(style?.borderBottomWidth ?? "");
    overlay.style.left = `${field.offsetLeft}px`;
    overlay.style.top = `${field.offsetTop}px`;
    overlay.style.width = `${field.clientWidth + bx}px`;
    overlay.style.height = `${field.clientHeight + by}px`;
    // the trailing mark keeps a last empty line, as the field has one after a line break
    text.data = `${value}\u200b`;
    if (text.parentNode !== overlay) overlay.replaceChildren(text);
    overlay.scrollTop = field.scrollTop;
    const base = overlay.getBoundingClientRect();
    const left = base.left + overlay.clientLeft - overlay.scrollLeft;
    const top = base.top + overlay.clientTop - overlay.scrollTop;
    const boxes: HTMLElement[] = [];
    for (const t of shown) {
      if (t.end > value.length) continue;
      const range = doc.createRange();
      range.setStart(text, t.start);
      range.setEnd(text, t.end);
      // one box for each line the token's characters stand on
      for (const r of range.getClientRects()) {
        const box = doc.createElement("span");
        box.className = "room-token";
        box.style.left = `${r.left - left}px`;
        box.style.top = `${r.top - top}px`;
        box.style.width = `${r.width}px`;
        box.style.height = `${r.height}px`;
        boxes.push(box);
      }
    }
    overlay.replaceChildren(...boxes, text);
    overlay.scrollTop = field.scrollTop;
  };

  /** The field's value read again: the tokens moved across the edit, a pick's made, and after undo or redo, the
   * tokens' texts found again. */
  const read = (history: boolean) => {
    const next = field.value;
    if (next === value && pending === null && !history) return;
    let moved = shiftTokens(value, next, tokens);
    value = next;
    if (pending !== null) {
      moved = withPick(next, moved, pending);
      pending = null;
    }
    tokens = history ? scanTokens(next, listed, moved) : moved;
    draw();
  };

  const onInput = (e: Event) => {
    const kind = (e as InputEvent).inputType ?? "";
    read(kind === "historyUndo" || kind === "historyRedo");
  };

  /** Selects the range and deletes it through the engine, so undo brings it back. */
  const remove = (
    t: { start: number; end: number },
    kind: "deleteContentBackward" | "deleteContentForward",
  ) => {
    const was = field.value;
    field.setSelectionRange(t.start, t.end);
    const ok = typeof doc.execCommand === "function" && doc.execCommand("delete");
    if (!ok || field.value === was) {
      // no undo-keeping edit in this engine (happy-dom): take the words out, and tell the field's listeners
      field.setRangeText("", t.start, t.end, "end");
      field.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: kind }));
    }
  };
  const mac = /Mac|iPhone|iPad/.test(view?.navigator.platform ?? "");

  const onKey = (e: KeyboardEvent) => {
    if (composing.current || e.isComposing || e.keyCode === 229 || e.defaultPrevented) return;
    const at = field.selectionStart;
    if (at !== field.selectionEnd) return;
    const live = standing(value, tokens);
    // a word delete, Option or Ctrl with Backspace or Delete, that would cut into a token takes it whole, as
    // Backspace does; macOS's Ctrl+H is Backspace. Cmd with Backspace deletes to the line's start, whole tokens only
    if ((e.altKey || e.ctrlKey) && !e.metaKey && (e.key === "Backspace" || e.key === "Delete")) {
      const forward = e.key === "Delete";
      const r = wordDelete(value, live, at, forward);
      if (r === null) return;
      e.preventDefault();
      remove(r, forward ? "deleteContentForward" : "deleteContentBackward");
      return;
    }
    const ctrlH = mac && e.ctrlKey && !e.altKey && !e.metaKey && e.key.toLowerCase() === "h";
    if (ctrlH) {
      const t = tokenEndingAt(live, at);
      if (t !== undefined) {
        e.preventDefault();
        remove(t, "deleteContentBackward");
      }
      return;
    }
    if (e.altKey || e.metaKey || e.ctrlKey) return;
    const before = tokenEndingAt(live, at);
    const after = tokenStartingAt(live, at);
    if (e.key === "Backspace" && before !== undefined) {
      e.preventDefault();
      remove(before, "deleteContentBackward");
    } else if (e.key === "Delete" && after !== undefined) {
      e.preventDefault();
      remove(after, "deleteContentForward");
    } else if (e.key === "ArrowLeft" && !e.shiftKey && before !== undefined) {
      e.preventDefault();
      field.setSelectionRange(before.start, before.start);
    } else if (e.key === "ArrowRight" && !e.shiftKey && after !== undefined) {
      e.preventDefault();
      field.setSelectionRange(after.end, after.end);
    }
  };

  // the selection as it last rested, so an end that moves into a token goes on the way it was going
  let last: { start: number; end: number } | null = null;
  /** The caret out of any token it stands in, a selection's ends out of tokens the way they travelled. */
  const snap = () => {
    if (composing.current || doc.activeElement !== field) return;
    if (tokens.length > 0) {
      // a selection change can come before the input event that moved the words under it
      if (field.value !== value) read(false);
      const to = snapped(standing(value, tokens), field.selectionStart, field.selectionEnd, last);
      if (to !== null)
        field.setSelectionRange(to.start, to.end, field.selectionDirection ?? undefined);
    }
    last = { start: field.selectionStart, end: field.selectionEnd };
  };

  const onScroll = () => {
    if (tokens.length > 0) overlay.scrollTop = field.scrollTop;
  };
  const resized = typeof ResizeObserver === "function" ? new ResizeObserver(() => draw()) : null;
  resized?.observe(field);
  const fonts = (doc as Document & { fonts?: FontFaceSet }).fonts;

  field.addEventListener("input", onInput);
  field.addEventListener("keydown", onKey);
  field.addEventListener("keyup", snap);
  field.addEventListener("pointerup", snap);
  field.addEventListener("scroll", onScroll);
  doc.addEventListener("selectionchange", snap);
  fonts?.addEventListener?.("loadingdone", draw);
  draw();

  return {
    now: () => standing(value, tokens),
    held: () => tokens,
    expect: (start, t) => {
      pending = { start, end: start + t.length, text: t };
    },
    texts: (next) => {
      listed = next;
      const kept = stillListed(tokens, next);
      if (kept.length === tokens.length) return;
      tokens = kept;
      draw();
    },
    sync: () => read(false),
    stop: () => {
      field.removeEventListener("input", onInput);
      field.removeEventListener("keydown", onKey);
      field.removeEventListener("keyup", snap);
      field.removeEventListener("pointerup", snap);
      field.removeEventListener("scroll", onScroll);
      doc.removeEventListener("selectionchange", snap);
      fonts?.removeEventListener?.("loadingdone", draw);
      resized?.disconnect();
      overlay.replaceChildren();
    },
  };
}
