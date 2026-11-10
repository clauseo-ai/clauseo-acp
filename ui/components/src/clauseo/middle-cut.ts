// A file's name on a tile, in at most two lines: a name too long for them loses letters from its middle, never its
// last letters and extension ("Statement of claim… final.pdf"). The span's lines are clamped in CSS (wall.css,
// .tile-name-text); this fits the words to them, measuring the span itself, again whenever its width changes. The span's
// text is set here, not by React, so a measure never fights a render. A cut name is for the eye: the span is hidden
// from assistive technology and the whole name stands in a sibling only it reads. Nothing to measure (no layout, as in
// a test) leaves the name whole.
import { useLayoutEffect, type RefObject } from "react";

/** How many of a name's last letters stay with its extension. */
const TAIL = 6;

/** A name's two parts: what may lose letters, and its last letters with the extension, which always show. A name with
 * no extension keeps its last letters alone. */
export function nameParts(name: string): { head: string; tail: string } {
  const dot = name.lastIndexOf(".");
  const ext = dot > 0 && name.length - dot <= 8 ? dot : name.length;
  const cut = Math.max(0, ext - TAIL);
  return { head: name.slice(0, cut), tail: name.slice(cut) };
}

/** What `fit` needs of the span: its text, and whether that text overflows its clamped lines. */
export type Fitted = {
  textContent: string | null;
  readonly scrollHeight: number;
  readonly clientHeight: number;
};

const overflows = (el: Fitted) => el.scrollHeight > el.clientHeight + 1;

/** Sets `el`'s text to `name`, or, when that overflows, to as much of its head as fits, an ellipsis and its tail;
 * whether it cut. */
export function fit(el: Fitted, name: string): boolean {
  el.textContent = name;
  if (!overflows(el)) return false;
  const { head, tail } = nameParts(name);
  const shown = (n: number) => `${head.slice(0, n).trimEnd()}…${tail}`;
  // the most letters of the head that still fit: a binary search, each step one measure
  let lo = 0;
  let hi = head.length - 1;
  while (lo < hi) {
    const mid = Math.ceil((lo + hi) / 2);
    el.textContent = shown(mid);
    if (overflows(el)) hi = mid - 1;
    else lo = mid;
  }
  el.textContent = shown(lo);
  return true;
}

/** Shows `name` in `el`, fitted; a cut name hides `el` from assistive technology, and `whole` holds the name. */
function show(el: HTMLElement, whole: HTMLElement, name: string): void {
  const cut = fit(el, name);
  if (cut) el.setAttribute("aria-hidden", "true");
  else el.removeAttribute("aria-hidden");
  whole.textContent = cut ? name : "";
}

/** Keeps the span at `ref` showing `name`, fitted to its lines now and at each change of its width, with the
 * whole name in `whole` while it is cut. */
export function useMiddleCut(
  ref: RefObject<HTMLElement | null>,
  whole: RefObject<HTMLElement | null>,
  name: string,
): void {
  useLayoutEffect(() => {
    const el = ref.current;
    const all = whole.current;
    if (el === null || all === null) return;
    show(el, all, name);
    if (typeof ResizeObserver === "undefined") return;
    let width = el.clientWidth;
    const watch = new ResizeObserver(() => {
      if (el.clientWidth === width) return;
      width = el.clientWidth;
      show(el, all, name);
    });
    watch.observe(el);
    return () => watch.disconnect();
  }, [ref, whole, name]);
}
