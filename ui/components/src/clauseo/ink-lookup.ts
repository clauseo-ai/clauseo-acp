// The nib's place along the ink, read from points the script works out once from the path's own text: never from the
// page's geometry. Blink measures the whole path again on every getPointAtLength (0.9 ms a read for the pen's 264
// curves in Chrome 155 on an M-series Mac), so the table the pen once sampled from it, 2,380 reads, held the main
// thread 2.1 s there as each pen first showed, and about 8 s on the 2-vCPU Windows VM, where the first run's
// "Setting up" showed that late (clauseo-acp-private/measurements/webview-stall).

/** Straight pieces each curve is cut into: the nib then sits within 0.006 path units of where Chrome puts it
 *  (0.003 px at the row's size), once lengths are scaled to the page's measure (pen-mark.tsx scales them). */
const PIECES = 32;

/** A path's points, with the length walked to each. */
export type InkPoints = {
  /** The path's length in the script's measure: its pieces' sum, under the curves' own by 0.02% for the pen. */
  length: number;
  /** The point `k` along the path, by straight steps between its points; a `k` outside [0, length] reads as the
   *  nearest end. */
  at: (k: number) => { x: number; y: number };
};

/** The points of `d`: an `M x y`, then relative cubics only (`c dx1 dy1 dx2 dy2 dx dy` each), as the pen's ink is.
 *  Any other command throws. Cheap: under 1 ms for the pen's 264 curves. */
export function inkPoints(d: string): InkPoints {
  const words = d.trim().split(/[\s,]+/);
  const xs: number[] = [];
  const ys: number[] = [];
  const ls: number[] = [];
  let x = 0;
  let y = 0;
  let walked = 0;
  let i = 0;
  const num = () => {
    const value = Number(words[i++]);
    if (!Number.isFinite(value)) throw new Error(`inkPoints: not a number at word ${i - 1}`);
    return value;
  };
  if (words[i++] !== "M") throw new Error("inkPoints: the path must start with M");
  x = num();
  y = num();
  xs.push(x);
  ys.push(y);
  ls.push(0);
  while (i < words.length) {
    const command = words[i++];
    if (command !== "c") throw new Error(`inkPoints: only relative cubics, not ${String(command)}`);
    const x1 = x + num();
    const y1 = y + num();
    const x2 = x + num();
    const y2 = y + num();
    const x3 = x + num();
    const y3 = y + num();
    let px = x;
    let py = y;
    for (let s = 1; s <= PIECES; s++) {
      const u = s / PIECES;
      const v = 1 - u;
      const qx = v * v * v * x + 3 * v * v * u * x1 + 3 * v * u * u * x2 + u * u * u * x3;
      const qy = v * v * v * y + 3 * v * v * u * y1 + 3 * v * u * u * y2 + u * u * u * y3;
      walked += Math.hypot(qx - px, qy - py);
      xs.push(qx);
      ys.push(qy);
      ls.push(walked);
      px = qx;
      py = qy;
    }
    x = x3;
    y = y3;
  }
  const last = ls.length - 1;
  return {
    length: walked,
    at: (k) => {
      if (!(k > 0)) return { x: xs[0]!, y: ys[0]! };
      if (k >= walked) return { x: xs[last]!, y: ys[last]! };
      // the last point walked to before k, by halving
      let lo = 0;
      let hi = last;
      while (hi - lo > 1) {
        const mid = (lo + hi) >> 1;
        if (ls[mid]! < k) lo = mid;
        else hi = mid;
      }
      const span = ls[hi]! - ls[lo]!;
      const t = span > 0 ? (k - ls[lo]!) / span : 0;
      return { x: xs[lo]! + (xs[hi]! - xs[lo]!) * t, y: ys[lo]! + (ys[hi]! - ys[lo]!) * t };
    },
  };
}
