// The page's clocks, behind one seam so the recorders' tests run frames by hand.

export interface Clock {
  /** ms on the page's performance clock (`performance.now()`). */
  now(): number;
  /** The performance clock's zero as ms since the Unix epoch (`performance.timeOrigin`). */
  readonly timeOrigin: number;
  /** Runs `run` at the next animation frame with the frame's timestamp. */
  frame(run: (timestamp: number) => void): void;
  /** Runs `run` as a task of its own (a MessageChannel message). Posted from an animation frame callback, it runs once
   * that frame's rendering steps are done: the earliest moment script can see that the frame was painted. */
  post(run: () => void): void;
}

/** One MessageChannel for every `post`: messages run in the order they were posted. */
function poster(): (run: () => void) => void {
  let channel: MessageChannel | null = null;
  const waiting: (() => void)[] = [];
  return (run) => {
    if (channel === null) {
      channel = new MessageChannel();
      channel.port1.onmessage = () => waiting.shift()?.();
    }
    waiting.push(run);
    channel.port2.postMessage(null);
  };
}

export const pageClock: Clock = {
  now: () => performance.now(),
  get timeOrigin() {
    return performance.timeOrigin;
  },
  frame: (run) => {
    requestAnimationFrame(run);
  },
  post: poster(),
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
