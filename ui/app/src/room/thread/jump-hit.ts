// Where in a found row the found words are (D-071): a Find result knows its entry and the snippet the host cut around
// the words it found, so a jump to a long answer can land on those words, not only on the answer's first line. The
// words are looked for in the row as drawn, by the snippet's found parts (as the result marks them, find-results.tsx),
// the snippet's words around them telling one occurrence from another. Nothing here runs at rest: a jump asks once.
import type { SnippetPart } from "../../transport/protocol.ts";

/** The smallest block that holds found words: a table's cell, a list's item, a paragraph, a heading, a quote. */
const BLOCK = "td, th, li, p, h1, h2, h3, h4, h5, h6, pre, blockquote, dd, dt";

/** How far, in characters, the row's words around found ones are read against the snippet's. */
const NEAR = 80;

const words = (text: string): string[] => text.split(/\s+/u).filter((w) => w.length > 0);
const escape = (word: string) => word.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
/** Letters and digits only, in lower case: the snippet is cut from the words as written (a table's pipes, a bold's
 * stars), the row from the words as drawn. */
const bare = (text: string) => text.toLocaleLowerCase().replace(/[^\p{L}\p{N}]+/gu, "");

/** How many characters `a` and `b` share at their ends (`back`) or their starts. */
function shared(a: string, b: string, back: boolean): number {
  let n = 0;
  const at = (s: string, i: number) => (back ? s[s.length - 1 - i] : s[i]);
  while (n < a.length && n < b.length && at(a, n) === at(b, n)) n++;
  return n;
}

/** The element in `row` that holds the first found part of `parts`, as the host found it; null when the row doesn't
 * show those words (an entry folded into a step group, or words the drawing changed). */
export function hitIn(row: Element, parts: readonly SnippetPart[]): Element | null {
  const first = parts.findIndex((p) => p.hit && words(p.text).length > 0);
  if (first < 0) return null;
  const nodes: Text[] = [];
  const starts: number[] = [];
  let text = "";
  const walk = row.ownerDocument.createTreeWalker(row, NodeFilter.SHOW_TEXT);
  for (let n = walk.nextNode(); n !== null; n = walk.nextNode()) {
    nodes.push(n as Text);
    starts.push(text.length);
    text += n.nodeValue ?? "";
  }
  const found = new RegExp(
    words(parts[first]?.text ?? "")
      .map(escape)
      .join("\\s+"),
    "giu",
  );
  const before = bare(
    parts
      .slice(0, first)
      .map((p) => p.text)
      .join(""),
  );
  const after = bare(
    parts
      .slice(first + 1)
      .map((p) => p.text)
      .join(""),
  );
  // the occurrence whose neighbours, read outward, run furthest as the snippet's do; the first among equals
  let best = -1;
  let score = -1;
  for (const m of text.matchAll(found)) {
    const at = m.index;
    const lead = bare(text.slice(Math.max(0, at - NEAR), at));
    const tail = bare(text.slice(at + m[0].length, at + m[0].length + NEAR));
    const s = shared(lead, before, true) + shared(tail, after, false);
    if (s > score) [best, score] = [at, s];
  }
  if (best < 0) return null;
  let i = starts.length - 1;
  while (i > 0 && (starts[i] ?? 0) > best) i--;
  const holder = nodes[i]?.parentElement;
  if (holder === null || holder === undefined) return null;
  const block = holder.closest(BLOCK);
  return block !== null && row.contains(block) ? block : holder;
}

/** The band of the window the found words can be read in, top and bottom in px from the viewport's top. */
export type Band = { top: number; bottom: number };

/** The space kept between found words and the band's edges. */
export const HIT_MARGIN = 8;

/** How far, in px, to scroll so found words at `hit` (from the viewport's top, as the band) read in `band`: none
 * when they are already clear of its edges by `margin`; else centred in it, or, taller than it, their top at its top. */
export function nudge(hit: Band, band: Band, margin = HIT_MARGIN): number {
  if (hit.top >= band.top + margin && hit.bottom <= band.bottom - margin) return 0;
  if (hit.bottom - hit.top + 2 * margin > band.bottom - band.top) {
    return hit.top - (band.top + margin);
  }
  return Math.round((hit.top + hit.bottom) / 2 - (band.top + band.bottom) / 2);
}

/** The band between the pinned question's band (`clearance` below the top of the thread's view) and the composer,
 * for the thread scroller `box`, or the page where the page scrolls (a narrow window, where the composer docks over
 * the thread's foot). */
export function bandOf(box: HTMLElement | null, clearance: number): Band {
  const view = { top: 0, bottom: window.innerHeight };
  const own = box !== null && box.scrollHeight > box.clientHeight;
  const rect = own ? box.getBoundingClientRect() : view;
  let top = Math.max(view.top, rect.top) + clearance;
  let bottom = Math.min(view.bottom, rect.bottom);
  const composer = document.querySelector(".room-composer")?.getBoundingClientRect();
  if (composer !== undefined && composer.top > top && composer.top < bottom) bottom = composer.top;
  if (bottom < top) top = bottom;
  return { top, bottom };
}

/** Scrolls by `dy` px whatever scrolls the thread: the scroller `box`, else the page. */
export function scrollThreadBy(box: HTMLElement | null, dy: number): void {
  if (dy === 0) return;
  if (box !== null && box.scrollHeight > box.clientHeight) box.scrollTop += dy;
  else window.scrollBy(0, dy);
}
