// The room's small pieces of logic, apart from the DOM, so the tests can hold them.

/* ---------- Citations show whole rows only ---------- */

export type Span = { top: number; bottom: number };

/** How many rows, in order, stand whole between top and foot. A row that starts above the top or runs past the foot
 * isn't whole; half a pixel of rounding is allowed either way. Rows after the first that doesn't fit don't count. */
export function wholeRowsWithin(rows: readonly Span[], top: number, foot: number): number {
  let n = 0;
  for (const r of rows) {
    if (r.top < top - 0.5 || r.bottom > foot + 0.5) break;
    n++;
  }
  return n;
}

/** The citations' fit on the wall. Every row whole: all of them, and no "N more" strip. Otherwise as many whole rows
 * as stand above the strip, which takes strip px under the last of them. */
export function fitCitations(rows: readonly Span[], top: number, foot: number, strip: number) {
  if (wholeRowsWithin(rows, top, foot) === rows.length) return { shown: rows.length, more: 0 };
  const shown = wholeRowsWithin(rows, top, foot - strip);
  return { shown, more: rows.length - shown };
}

/* ---------- Accept: only the lawyer accepts ---------- */

/** The answer's foot: not yet accepted, or accepted at a time. Weight says which, with no other marker. */
export type AcceptState = "pending" | "accepted";
export type AcceptEvent = "accept" | "undo";

/** Accept takes a pending answer to accepted; Undo, from the foot or the toolbar, takes it back. Anything else
 * leaves it as it is. */
export function acceptNext(state: AcceptState, event: AcceptEvent): AcceptState {
  if (event === "accept") return "accepted";
  return state === "accepted" ? "pending" : state;
}

/* ---------- the status line never truncates ---------- */

/** How the status line stands: everything on one line; the ready hint without its words, keeping its key cap; or
 * wrapped to two lines, the hint still without its words where it has a key cap to keep. */
export type StatusFit = "full" | "tight" | "wrapped";

/** The next fit to try when the line is still too long: full, then tight where there's a key cap to keep, then
 * wrapped. Under 1000 px there's no key cap, so the hint keeps its words and the line wraps. */
export function nextStatusFit(fit: StatusFit, hasKeyCap: boolean): StatusFit {
  if (fit === "full" && hasKeyCap) return "tight";
  return "wrapped";
}

/* ---------- the accept settle: a weight paired with a width per step ---------- */

export const SETTLE_WEIGHTS = [400, 450, 500, 550, 600] as const;

/** For each weight, the widest stretch (75% up to base) whose measure stays at or under the goal, by bisection over
 * a measure function. 400 keeps the base. */
export function settleCurve(base: number, measure: (weight: number, stretch: number) => number) {
  const goal = measure(400, base) * 0.997;
  return SETTLE_WEIGHTS.map((w) => {
    if (w === 400) return { w, s: base };
    let lo = 75;
    let hi = base;
    for (let i = 0; i < 10; i++) {
      const mid = (lo + hi) / 2;
      if (measure(w, mid) > goal) hi = mid;
      else lo = mid;
    }
    return { w, s: Math.round(lo * 100) / 100 };
  });
}
