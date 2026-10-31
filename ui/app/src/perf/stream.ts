// Records one stream (contract §5, StreamSample), between `start` and `stop`:
//
// - rAF timestamps: a frame loop that runs only while recording.
// - React commits per frame: the loop reads `__CLAUSEO_PERF__.reactCommits` each frame; the difference from the last
//   frame is the commits of the frame between (each gap between two loop callbacks holds one frame's rAF work, where
//   the store flushes). Mean and max over frames with at least one commit.
// - Long animation frames: a `long-animation-frame` PerformanceObserver, null where the engine lacks the type.
// - Chunk to paint: for each `timed` event carrying patches for a thread stream, a double rAF from its arrival. The
//   store's flush for that event was scheduled (as an animation frame) when the transport handed the event on, before
//   the hook ran, so the first callback runs right after the flush and React's commit in the same frame; the second
//   runs once that frame was painted. The sample is that moment (`timeOrigin + now()`) minus `received_ms`.
// - First token on screen: from the `send_prompt` command being issued (the transport's `dispatch` called) to the
//   frame that painted the first answer text. After each store flush the recorder checks whether the prompted thread's
//   answer text grew past what it held at the send; the first time it did, the next animation frame (the flush ran in
//   this frame's rAF work, so that is after the paint) closes the sample. (A flush from the store's 100 ms fallback,
//   which runs only when the page gets no frames, would close it one paint early; a visible window gets frames.)
// - Answer text bytes: UTF-8 bytes of the prompted thread's answer texts in the view store at stop.
import type { ChannelEvent } from "../transport/channel.ts";
import { epochNow, type Clock } from "./clock.ts";
import type { StreamSample } from "./env.ts";

/** A long-animation-frame observer while recording; `takeDurations` stops it and gives the durations seen. */
export type LongFrames = { start(): void; takeDurations(): number[] };

export type StreamSources = {
  clock: Clock;
  /** React commits so far. */
  reactCommits: () => number;
  /** Null where the engine has no long-animation-frame entries. */
  longFrames: LongFrames | null;
  /** UTF-8 bytes of the answer texts of `stream` in the view store now. */
  answerBytes: (stream: string) => number;
};

/** The thread stream a `timed` event changes, or null when it changes no transcript. */
function threadPatched(event: ChannelEvent): boolean {
  if (!("patches" in event) || event.patches === undefined) return false;
  return event.patches.stream.startsWith("thread:") && event.patches.patches.length > 0;
}

/** Frames `stop` waits at most for chunk samples still waiting for their second frame. */
const STOP_FRAMES = 4;

export class StreamRecorder {
  readonly #s: StreamSources;
  #recording = false;
  /** Bumped by each `start`, so a frame loop or sample of an earlier recording drops out. */
  #run = 0;
  #raf: number[] = [];
  #chunks: number[] = [];
  #pendingChunks = 0;
  #lastCommits = 0;
  #commitFrames = 0;
  #commitSum = 0;
  #commitMax = 0;
  #stream: string | null = null;
  #sentAt: number | null = null;
  #bytesAtSend = 0;
  #firstToken: number | null = null;
  #firstTokenWaiting = false;

  constructor(sources: StreamSources) {
    this.#s = sources;
  }

  get recording(): boolean {
    return this.#recording;
  }

  start(): void {
    this.#run += 1;
    this.#recording = true;
    this.#raf = [];
    this.#chunks = [];
    this.#pendingChunks = 0;
    this.#lastCommits = this.#s.reactCommits();
    this.#commitFrames = 0;
    this.#commitSum = 0;
    this.#commitMax = 0;
    this.#stream = null;
    this.#sentAt = null;
    this.#firstToken = null;
    this.#firstTokenWaiting = false;
    this.#s.longFrames?.start();
    const run = this.#run;
    const loop = (timestamp: number) => {
      if (!this.#recording || run !== this.#run) return;
      this.#raf.push(timestamp);
      const commits = this.#s.reactCommits();
      const frame = commits - this.#lastCommits;
      this.#lastCommits = commits;
      if (frame > 0) {
        this.#commitFrames += 1;
        this.#commitSum += frame;
        this.#commitMax = Math.max(this.#commitMax, frame);
      }
      this.#s.clock.frame(loop);
    };
    this.#s.clock.frame(loop);
  }

  /** The transport's `send_prompt` for `thread` was issued. The first one after `start` counts. */
  onPrompt(thread: string): void {
    if (!this.#recording || this.#sentAt !== null) return;
    this.#sentAt = this.#s.clock.now();
    this.#stream = `thread:${thread}`;
    this.#bytesAtSend = this.#s.answerBytes(this.#stream);
  }

  /** A timed event was handed on; `receivedMs` is the bridge's receive time (epoch ms). */
  onTimed(event: ChannelEvent, receivedMs: number): void {
    if (!this.#recording || !threadPatched(event)) return;
    const run = this.#run;
    this.#pendingChunks += 1;
    const { clock } = this.#s;
    clock.frame(() =>
      clock.frame(() => {
        if (run !== this.#run) return;
        this.#pendingChunks -= 1;
        // Two clocks (the bridge's system clock, the page's timeOrigin + now) can disagree by about a millisecond;
        // a sample below 0 is that disagreement, kept as 0 so the run stays valid (metrics.ts refuses negatives).
        this.#chunks.push(Math.max(0, epochNow(clock) - receivedMs));
      }),
    );
  }

  /** The view store flushed a change. */
  onStoreChange(): void {
    if (!this.#recording || this.#sentAt === null || this.#stream === null) return;
    if (this.#firstToken !== null || this.#firstTokenWaiting) return;
    if (this.#s.answerBytes(this.#stream) <= this.#bytesAtSend) return;
    this.#firstTokenWaiting = true;
    const run = this.#run;
    const sentAt = this.#sentAt;
    this.#s.clock.frame(() => {
      if (run !== this.#run) return;
      this.#firstToken = this.#s.clock.now() - sentAt;
      this.#firstTokenWaiting = false;
    });
  }

  /** Stops, after at most a few frames for samples still waiting to be painted, and gives the sample. */
  async stop(): Promise<StreamSample> {
    for (let i = 0; i < STOP_FRAMES && (this.#pendingChunks > 0 || this.#firstTokenWaiting); i++) {
      await new Promise<void>((done) => this.#s.clock.frame(() => done()));
    }
    this.#recording = false;
    this.#run += 1;
    const longFrames = this.#s.longFrames?.takeDurations() ?? null;
    return {
      first_token_on_screen_ms: this.#firstToken,
      chunk_to_paint_ms: this.#chunks,
      raf_timestamps_ms: this.#raf,
      long_animation_frames_ms: longFrames,
      react_commits_per_frame:
        this.#commitFrames === 0 ? null : this.#commitSum / this.#commitFrames,
      react_commits_per_frame_max: this.#commitFrames === 0 ? null : this.#commitMax,
      answer_text_bytes: this.#stream === null ? 0 : this.#s.answerBytes(this.#stream),
    };
  }
}

/** The page's long-animation-frame observer, or null where `PerformanceObserver` lacks the entry type. */
export function pageLongFrames(): LongFrames | null {
  if (typeof PerformanceObserver === "undefined") return null;
  if (!PerformanceObserver.supportedEntryTypes.includes("long-animation-frame")) return null;
  let durations: number[] = [];
  const observer = new PerformanceObserver((list) => {
    for (const e of list.getEntries()) durations.push(e.duration);
  });
  return {
    start() {
      durations = [];
      observer.observe({ type: "long-animation-frame" });
    },
    takeDurations() {
      for (const e of observer.takeRecords()) durations.push(e.duration);
      observer.disconnect();
      return durations;
    },
  };
}
