// The perf module's shapes (R13 contract §3 and §5): what the e2e and marks builds' init script puts on the window
// before any page script, and the API the module sets once loaded. Types only; nothing here runs.

/** Set by the init script of the e2e and marks builds, never in release (contract §3). */
export interface PerfInit {
  /** The app's start, ms since the Unix epoch, taken at the first line of the desktop crate's `run()`. */
  t0EpochMs: number;
  /** True only in the marks build: the page plays the scoreboard session itself (autoplay.ts). */
  autoplay: boolean;
  /** React commits so far, counted by the init script's `__REACT_DEVTOOLS_GLOBAL_HOOK__.onCommitFiberRoot`. */
  reactCommits: number;
}

/** What one recorded stream gives (contract §5). */
export interface StreamSample {
  /** The prompt command issued to the first painted frame showing the turn's text, thought or answer (stream.ts). */
  first_token_on_screen_ms: number | null;
  /** Per timed event that changed a thread's transcript: received_ms to just after the frame showing it was rendered
   * (stream.ts), epoch ms both. */
  chunk_to_paint_ms: number[];
  /** The same events stamped the earlier way, at the next frame's animation frame callback (about a frame later than
   * `chunk_to_paint_ms`); for comparison runs, not in RawRun. */
  chunk_to_paint_double_raf_ms: number[];
  /** Every rAF timestamp while recording (ms on the page's performance clock). */
  raf_timestamps_ms: number[];
  /** Durations of long-animation-frame entries while recording; null where unsupported (WebKit). */
  long_animation_frames_ms: number[] | null;
  /** Mean React commits over frames that had at least one commit, while recording; null with none. */
  react_commits_per_frame: number | null;
  react_commits_per_frame_max: number | null;
  /** UTF-8 bytes of the answer text the prompted thread's view holds at stop. */
  answer_text_bytes: number;
}

/** What `stopKeystrokes` gives. One sample per composer keydown in each list; null lists where Event Timing is
 * unsupported (WebKit). `entry_share` (beyond the contract, read by nobody yet) is the share of keystrokes whose
 * samples came from Event Timing entries rather than the fallbacks keystrokes.ts describes. */
export interface KeystrokeSample {
  latency_ms: number[] | null;
  handler_ms: number[] | null;
  entry_share: number | null;
}

/** `window.__clauseoPerf` (contract §5). */
export interface ClauseoPerf {
  /** ms since t0 of the marks, null until reached; marks.ts defines each. */
  marks(): { first_paint_ms: number | null; interactive_ms: number | null };
  startStream(): void;
  stopStream(): Promise<StreamSample>;
  startKeystrokes(): void;
  stopKeystrokes(): KeystrokeSample;
  /** `perf_channel_bytes`, through invoke. */
  channelBytes(): Promise<number>;
}

declare global {
  interface Window {
    __CLAUSEO_PERF__?: PerfInit;
    __clauseoPerf?: ClauseoPerf;
  }
}
