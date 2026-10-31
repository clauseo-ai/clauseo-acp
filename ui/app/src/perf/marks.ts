// The start-up marks (contract §5), as ms since `t0EpochMs` (the desktop crate's `run()`):
//
// - `first_paint`: the paint timing entry `first-paint` (Chromium), else `first-contentful-paint` (WebKit has only
//   that one), converted with `timeOrigin`. If the engine has neither, the double rAF after this module started
//   watching, which is later than the real first paint (the module loads after the first render).
// - `interactive`: the room is usable. The matter picker (`new-matter`) and the composer are in the DOM and the
//   transport's channel is registered (`open_stream` answered); the mark is taken at the next animation frame after
//   the frame in which all three were first seen, so that frame has been painted. The composer stays disabled until a
//   thread is open, which is not part of `interactive`: picking a matter is the lawyer's next action.
import { epochNow, type Clock } from "./clock.ts";

export type MarkSources = {
  t0EpochMs: number;
  clock: Clock;
  /** The page's paint timing entries (`performance.getEntriesByType("paint")`). */
  paintEntries: () => readonly { name: string; startTime: number }[];
  /** Whether the matter picker and the composer are rendered. */
  rendered: () => boolean;
  /** Resolves once the transport's channel is registered. */
  connected: Promise<void>;
};

export type Marks = { first_paint_ms: number | null; interactive_ms: number | null };

/** Starts watching; the returned function reads the marks reached so far. */
export function watchMarks(s: MarkSources): () => Marks {
  let fallbackPaint: number | null = null;
  let interactive: number | null = null;
  let isConnected = false;
  const sinceT0 = () => epochNow(s.clock) - s.t0EpochMs;

  s.clock.frame(() => s.clock.frame(() => (fallbackPaint = sinceT0())));
  s.connected.then(
    () => (isConnected = true),
    () => {},
  );
  const poll = () => {
    if (isConnected && s.rendered()) s.clock.frame(() => (interactive = sinceT0()));
    else s.clock.frame(poll);
  };
  s.clock.frame(poll);

  return () => {
    const entries = s.paintEntries();
    const entry =
      entries.find((e) => e.name === "first-paint") ??
      entries.find((e) => e.name === "first-contentful-paint");
    const firstPaint =
      entry === undefined ? fallbackPaint : s.clock.timeOrigin + entry.startTime - s.t0EpochMs;
    return { first_paint_ms: firstPaint, interactive_ms: interactive };
  };
}
