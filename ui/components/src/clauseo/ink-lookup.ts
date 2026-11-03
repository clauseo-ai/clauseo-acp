// The nib's place along the ink, read from a table sampled once: a frame interpolates in it instead of walking the
// path with getPointAtLength, the largest part of a frame's script.

/** The spacing, in path units, of the table the nib's place is read from. */
const SAMPLE_STEP = 0.25;

/** A lookup of the ink's points from a table sampled once, every SAMPLE_STEP from k0 to k1, so a frame interpolates
 *  instead of walking the path. A k outside [k0, k1] reads as the nearest end. */
export function inkLookup(point: (k: number) => { x: number; y: number }, k0: number, k1: number) {
  const n = Math.ceil((k1 - k0) / SAMPLE_STEP) + 2;
  const xs: number[] = [];
  const ys: number[] = [];
  for (let j = 0; j < n; j++) {
    const q = point(k0 + j * SAMPLE_STEP);
    xs.push(q.x);
    ys.push(q.y);
  }
  return (k: number) => {
    const f = (Math.min(Math.max(k, k0), k1) - k0) / SAMPLE_STEP;
    const i = Math.floor(f);
    const t = f - i;
    return {
      x: xs[i]! + (xs[i + 1]! - xs[i]!) * t,
      y: ys[i]! + (ys[i + 1]! - ys[i]!) * t,
    };
  };
}
