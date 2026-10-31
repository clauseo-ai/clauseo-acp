// A clock whose animation frames run when a test says. Not shipped: only tests import it.
import type { Clock } from "./clock.ts";

export function fakeClock(timeOrigin = 1_700_000_000_000) {
  let now = 0;
  let queued: ((t: number) => void)[] = [];
  const clock: Clock = { now: () => now, timeOrigin, frame: (run) => void queued.push(run) };
  return {
    clock,
    /** Moves the clock without a frame. */
    advance: (ms: number) => void (now += ms),
    /** One frame, `ms` after the last: runs every callback queued before it. */
    frame: (ms = 16) => {
      now += ms;
      const run = queued;
      queued = [];
      for (const r of run) r(now);
    },
    waiting: () => queued.length,
  };
}
