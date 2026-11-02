// The `/` menu's rules, apart from the DOM where they can be: where a trigger stands, which commands a query matches,
// which row stays highlighted when the list changes, and where the menu stands by the caret. G1a's spike
// (measurements/g1a-indic-input/spike/src/mentions/logic.ts in the private repo) is the reference. The caret's place
// is measured here too, from a hidden copy of the field, the one function that needs a page.
import type { SlashCommand } from "./composer.tsx";

const WS = /\s/u;

/** A `/` trigger: where its slash stands and the query typed after it, up to the caret. */
export type Slash = { at: number; query: string };

/** The trigger the caret stands in, if any. The caret's run is the text since the last whitespace or the field's
 * start; a `/` counts only as the run's first character, so `a/` is no trigger and `x /re` is. */
export function slashAt(value: string, caret: number): Slash | null {
  let start = caret;
  while (start > 0 && !WS.test(value[start - 1] ?? "")) start -= 1;
  if (value[start] !== "/" || start >= caret) return null;
  return { at: start, query: value.slice(start + 1, caret) };
}

const fold = (s: string) => s.normalize("NFC").toLowerCase();

/** The commands whose name starts with the query, case and normalisation aside, in the agent's order. */
export function matching(commands: readonly SlashCommand[], query: string): SlashCommand[] {
  const q = fold(query);
  return commands.filter((c) => fold(c.name).startsWith(q));
}

/** The row to highlight in a new list: the one named as the highlighted row was, or the first. */
export function keepHighlight(items: readonly SlashCommand[], name: string | null): number {
  if (name === null) return 0;
  const i = items.findIndex((c) => c.name === name);
  return i === -1 ? 0 : i;
}

/** Where the run that starts at `from` ends: at the next whitespace, or the field's end. A pick replaces the whole
 * run, so `/re|view` picked as review is `/review `, not `/review view`. */
export function runEnd(value: string, from: number): number {
  let end = from;
  while (end < value.length && !WS.test(value[end] ?? "")) end += 1;
  return end;
}

/** The rows' element ids, from the list's id and each command's name, so a list replaced under the highlight says the
 * new row (aria-activedescendant changes with it). A name the agent lists twice gets a count after it. */
export function optionIds(list: string, items: readonly SlashCommand[]): string[] {
  const seen = new Map<string, number>();
  return items.map((c) => {
    const n = seen.get(c.name) ?? 0;
    seen.set(c.name, n + 1);
    const id = `${list}-${encodeURIComponent(c.name)}`;
    return n === 0 ? id : `${id}~${n}`;
  });
}

/** The text a pick puts in place of the trigger: the slash and the name, and a space unless one follows already. */
export function picked(command: SlashCommand, following: string | undefined): string {
  const text = `/${command.name}`;
  return following === undefined || !WS.test(following) ? `${text} ` : text;
}

/** The caret's line on the page: its x, and its line's top and bottom, in viewport px. */
export type Caret = { x: number; top: number; bottom: number };

/** Where the menu stands: its left edge at the caret's x and 4 px below the caret's line, or 4 px above it when the
 * window has no room below, as at the room's foot; kept 8 px inside the window, and as tall as the room it has. */
export function placeMenu(
  caret: Caret,
  menu: { width: number; height: number },
  view: { width: number; height: number },
): { left: number; top: number; maxHeight: number } {
  const gap = 4;
  const edge = 8;
  const below = view.height - edge - (caret.bottom + gap);
  const above = caret.top - gap - edge;
  const down = menu.height <= below || below >= above;
  const maxHeight = Math.max(0, Math.floor(down ? below : above));
  const height = Math.min(menu.height, maxHeight);
  const top = down ? caret.bottom + gap : caret.top - gap - height;
  const left = Math.max(edge, Math.min(caret.x, view.width - edge - menu.width));
  return { left: Math.round(left), top: Math.round(top), maxHeight };
}

/** What the hidden copy takes from the field, so its words wrap where the field's do. */
const COPIED = [
  "boxSizing",
  "borderTopWidth",
  "borderRightWidth",
  "borderBottomWidth",
  "borderLeftWidth",
  "borderStyle",
  "paddingTop",
  "paddingRight",
  "paddingBottom",
  "paddingLeft",
  "fontFamily",
  "fontSize",
  "fontStyle",
  "fontVariant",
  "fontWeight",
  "fontStretch",
  "fontKerning",
  "fontFeatureSettings",
  "fontVariationSettings",
  "lineHeight",
  "letterSpacing",
  "wordSpacing",
  "textTransform",
  "textIndent",
  "textAlign",
  "textRendering",
  "tabSize",
  "direction",
  "wordBreak",
  "overflowWrap",
  "hyphens",
  "transform",
] as const;

/** The caret's place, from a hidden copy of the field laid over it with a mark at the caret: the same face, size,
 * padding and wrapping, the field's client width (so a scrollbar can't move the wrapping) and its scroll. Built with
 * text nodes only. */
export function caretOf(field: HTMLTextAreaElement): Caret {
  const doc = field.ownerDocument;
  const style = getComputedStyle(field);
  const box = field.getBoundingClientRect();
  const copy = doc.createElement("div");
  for (const p of COPIED) copy.style[p] = style[p];
  const borders = parseFloat(style.borderLeftWidth) + parseFloat(style.borderRightWidth);
  copy.style.position = "fixed";
  copy.style.left = `${box.left}px`;
  copy.style.top = `${box.top}px`;
  copy.style.width = `${field.clientWidth + (Number.isFinite(borders) ? borders : 0)}px`;
  copy.style.height = `${box.height}px`;
  copy.style.overflow = "hidden";
  copy.style.whiteSpace = "pre-wrap";
  copy.style.visibility = "hidden";
  copy.style.pointerEvents = "none";
  copy.setAttribute("aria-hidden", "true");
  const at = field.selectionEnd;
  const mark = doc.createElement("span");
  mark.textContent = "\u200b";
  copy.append(
    doc.createTextNode(field.value.slice(0, at)),
    mark,
    doc.createTextNode(field.value.slice(at) + "\u200b"),
  );
  doc.body.append(copy);
  copy.scrollTop = field.scrollTop;
  const r = mark.getBoundingClientRect();
  copy.remove();
  return { x: r.left, top: r.top, bottom: r.bottom };
}
