// The page's clocks, behind one seam so the recorders' tests run frames by hand.

export interface Clock {
  /** ms on the page's performance clock (`performance.now()`). */
  now(): number;
  /** The performance clock's zero as ms since the Unix epoch (`performance.timeOrigin`). */
  readonly timeOrigin: number;
  /** Runs `run` at the next animation frame with the frame's timestamp. */
  frame(run: (timestamp: number) => void): void;
}

export const pageClock: Clock = {
  now: () => performance.now(),
  get timeOrigin() {
    return performance.timeOrigin;
  },
  frame: (run) => {
    requestAnimationFrame(run);
  },
};

/** Now as ms since the Unix epoch: `timeOrigin + now()`, the clock the bridge's `received_ms` is compared with. */
export const epochNow = (clock: Clock): number => clock.timeOrigin + clock.now();

/** Resolves at the `n`th animation frame from now. Two frames from a moment is the "double rAF": the second callback
 * runs once the frame after that moment has been painted. */
export function afterFrames(clock: Clock, n: number): Promise<void> {
  return new Promise((done) => {
    const step = (left: number) => (left === 0 ? done() : clock.frame(() => step(left - 1)));
    step(n);
  });
}
