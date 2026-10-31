// Records one stream (contract §5, StreamSample), between `start` and `stop`:
//
// - rAF timestamps: a frame loop that runs only while recording.
// - React commits per frame: the loop reads `__CLAUSEO_PERF__.reactCommits` each frame; the difference from the last
//   frame is the commits of the frame between (each gap between two loop callbacks holds one frame's rAF work, where
//   the store flushes). Mean and max over frames with at least one commit.
// - Long animation frames: a `long-animation-frame` PerformanceObserver, null where the engine lacks the type.
// - Chunk to paint, for each `timed` event carrying patches for a thread stream: the post-paint stamp. The store's
//   flush for that event was scheduled (as an animation frame) when the transport handed the event on, before the hook
//   ran, so the hook's own animation frame callback runs right after the flush and React's commit, in the same frame.
//   From there it posts a task (`Clock.post`, a MessageChannel message), which runs once that frame's rendering steps
//   are done; the sample is that moment (`timeOrigin + now()`) minus `received_ms`. What it measures: the bridge
//   receiving the event to the main thread having rendered the frame that shows it. Known error: it is early by the
//   compositor's share (raster, GPU, the display's vsync: up to about a frame before the pixels are on glass), and late
//   by any task queued before the message. `chunk_to_paint_double_raf_ms` keeps the earlier stamp, the next frame's
//   animation frame callback, which lands about a frame later, for comparison runs.
// - First token on screen: from the `send_prompt` command being issued (the transport's `onDispatch`) to the first
//   painted frame showing transcript text of the turn, thought or answer (the scenario's thoughts come first). After
//   each store flush the recorder checks whether the prompted thread's agent text grew past what it held at the send;
//   the first time it did, it posts a task from there (the flush runs in an animation frame callback), and the sample
//   closes when that task runs, after the frame's rendering: the same stamp as chunk to paint. (A flush from the
//   store's 100 ms fallback, which runs only when the page gets no frames, would close it before the paint.)
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
  /** UTF-8 bytes of the agent texts of `stream`, thoughts and answers, in the view store now. */
  transcriptTextBytes: (stream: string) => number;
};

/** The thread stream a `timed` event changes, or null when it changes no transcript. */
function threadPatched(event: ChannelEvent): boolean {
  if (!("patches" in event) || event.patches === undefined) return false;
  return event.patches.stream.startsWith("thread:") && event.patches.patches.length > 0;
}

/** Frames `stop` waits at most for samples still waiting for their paint. */
const STOP_FRAMES = 4;

export class StreamRecorder {
  readonly #s: StreamSources;
  #recording = false;
  /** Bumped by each `start`, so a frame loop or sample of an earlier recording drops out. */
  #run = 0;
  #raf: number[] = [];
  #chunks: number[] = [];
  #chunksDoubleRaf: number[] = [];
  /** Samples (of either stamp) still waiting for their paint. */
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
    this.#chunksDoubleRaf = [];
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
    this.#bytesAtSend = this.#s.transcriptTextBytes(this.#stream);
  }

  /** A timed event was handed on; `receivedMs` is the bridge's receive time (epoch ms). */
  onTimed(event: ChannelEvent, receivedMs: number): void {
    if (!this.#recording || !threadPatched(event)) return;
    const run = this.#run;
    const { clock } = this.#s;
    // Two clocks (the bridge's system clock, the page's timeOrigin + now) can disagree by about a millisecond; a
    // sample below 0 is that disagreement, kept as 0 so the run stays valid (metrics.ts refuses negatives).
    const since = () => Math.max(0, epochNow(clock) - receivedMs);
    this.#pendingChunks += 2;
    clock.frame(() => {
      clock.post(() => {
        if (run !== this.#run) return;
        this.#pendingChunks -= 1;
        this.#chunks.push(since());
      });
      clock.frame(() => {
        if (run !== this.#run) return;
        this.#pendingChunks -= 1;
        this.#chunksDoubleRaf.push(since());
      });
    });
  }

  /** The view store flushed a change. */
  onStoreChange(): void {
    if (!this.#recording || this.#sentAt === null || this.#stream === null) return;
    if (this.#firstToken !== null || this.#firstTokenWaiting) return;
    if (this.#s.transcriptTextBytes(this.#stream) <= this.#bytesAtSend) return;
    this.#firstTokenWaiting = true;
    const run = this.#run;
    const sentAt = this.#sentAt;
    this.#s.clock.post(() => {
      if (run !== this.#run) return;
      this.#firstToken = this.#s.clock.now() - sentAt;
      this.#firstTokenWaiting = false;
    });
  }

  /** UTF-8 bytes of the prompted thread's answer texts now; 0 before a prompt. */
  answerBytesNow(): number {
    return this.#stream === null ? 0 : this.#s.answerBytes(this.#stream);
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
      chunk_to_paint_double_raf_ms: this.#chunksDoubleRaf,
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
