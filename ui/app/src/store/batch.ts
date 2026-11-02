// When the store applies what has arrived: once a frame, or every 100 ms in lite (the design's stream store,
// design/03-system stream-store.ts). A test passes its own `Batch` to run frames by hand.

/** Schedules one flush; returns its cancel. */
export type Batch = (flush: () => void) => () => void;

/** The flushes waiting for the next frame, in the order they were scheduled. */
let waiting: (() => void)[] = [];
let raf = 0;
let late = 0;

/** Runs every waiting flush in one task, so the stores that change in a frame (the thread's view, the matters' rows)
 * notify React together and the frame commits once. A flush that throws doesn't stop the others; the first error is
 * thrown once they have all run. */
function runWaiting(): void {
  cancelAnimationFrame(raf);
  window.clearTimeout(late);
  raf = 0;
  late = 0;
  const flushes = waiting;
  waiting = [];
  let failed = false;
  let error: unknown;
  for (const flush of flushes) {
    try {
      flush();
    } catch (e) {
      if (!failed) error = e;
      failed = true;
    }
  }
  if (failed) throw error;
}

/** Once a frame, one animation frame callback for every store that uses it. A page that gets no frames (a hidden
 * window) still catches up every 100 ms, so the stream never stalls behind the screen. */
export const frameBatch: Batch = (flush) => {
  // its own function, so cancelling it never drops another store's flush of the same function
  const entry = () => flush();
  waiting.push(entry);
  if (raf === 0) {
    raf = requestAnimationFrame(runWaiting);
    late = window.setTimeout(runWaiting, 100);
  }
  return () => {
    waiting = waiting.filter((f) => f !== entry);
    if (waiting.length === 0 && raf !== 0) {
      cancelAnimationFrame(raf);
      window.clearTimeout(late);
      raf = 0;
      late = 0;
    }
  };
};

/** Every 100 ms, for lite. */
export const liteBatch: Batch = (flush) => {
  const id = window.setTimeout(flush, 100);
  return () => window.clearTimeout(id);
};
