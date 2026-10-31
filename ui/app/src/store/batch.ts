// When the store applies what has arrived: once a frame, or every 100 ms in lite (the design's stream store,
// design/03-system stream-store.ts). A test passes its own `Batch` to run frames by hand.

/** Schedules one flush; returns its cancel. */
export type Batch = (flush: () => void) => () => void;

/** Once a frame. A page that gets no frames (a hidden window) still catches up every 100 ms, so the stream never
 * stalls behind the screen. */
export const frameBatch: Batch = (flush) => {
  let raf = 0;
  let late = 0;
  const run = () => {
    cancelAnimationFrame(raf);
    window.clearTimeout(late);
    flush();
  };
  raf = requestAnimationFrame(run);
  late = window.setTimeout(run, 100);
  return () => {
    cancelAnimationFrame(raf);
    window.clearTimeout(late);
  };
};

/** Every 100 ms, for lite. */
export const liteBatch: Batch = (flush) => {
  const id = window.setTimeout(flush, 100);
  return () => window.clearTimeout(id);
};
