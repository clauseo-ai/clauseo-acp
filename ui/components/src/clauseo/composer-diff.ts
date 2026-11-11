// Where two of the field's values part, compared a block at a time: the field can hold 6 MiB, and a keystroke's diff
// compared one character at a time took 12 ms of each key after a 6 MiB paste (Chromium CPU profile); a block's
// equality is the engine's own compare.

const BLOCK = 4096;

/** How many characters `a` and `b` share at their starts, up to `max` (at most the shorter's length). */
export function sharedStart(a: string, b: string, max = Math.min(a.length, b.length)): number {
  let p = 0;
  while (p + BLOCK <= max && a.slice(p, p + BLOCK) === b.slice(p, p + BLOCK)) p += BLOCK;
  while (p < max && a.charCodeAt(p) === b.charCodeAt(p)) p += 1;
  return p;
}

/** How many characters `a` and `b` share at their ends, up to `max` (at most the shorter's length). */
export function sharedEnd(a: string, b: string, max = Math.min(a.length, b.length)): number {
  let s = 0;
  while (
    s + BLOCK <= max &&
    a.slice(a.length - s - BLOCK, a.length - s) === b.slice(b.length - s - BLOCK, b.length - s)
  )
    s += BLOCK;
  while (s < max && a.charCodeAt(a.length - 1 - s) === b.charCodeAt(b.length - 1 - s)) s += 1;
  return s;
}
