// A clock whose animation frames run when a test says. Not shipped: only tests import it.
import type { Clock } from "./clock.ts";

export function fakeClock(timeOrigin = 1_700_000_000_000) {
  let now = 0;
  let queued: ((t: number) => void)[] = [];
  let posted: (() => void)[] = [];
  const clock: Clock = {
    now: () => now,
    timeOrigin,
    frame: (run) => void queued.push(run),
    post: (run) => void posted.push(run),
  };
  const tasks = () => {
    while (posted.length > 0) {
      const run = posted;
      posted = [];
      for (const r of run) r();
    }
  };
  return {
    clock,
    /** Moves the clock without a frame. */
    advance: (ms: number) => void (now += ms),
    /** One frame, `ms` after the last: runs every callback queued before it, then, `paintMs` later (the frame's
     * rendering), the tasks posted so far. */
    frame: (ms = 16, paintMs = 0) => {
      now += ms;
      const run = queued;
      queued = [];
      for (const r of run) r(now);
      now += paintMs;
      tasks();
    },
    /** Runs the tasks posted so far, without a frame. */
    tasks,
    waiting: () => queued.length,
  };
}
