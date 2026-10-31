import { describe, expect, test } from "vitest";
import type { ChannelEvent } from "../transport/channel.ts";
import { fakeClock } from "./fake-clock.ts";
import { StreamRecorder, type LongFrames } from "./stream.ts";

const patches = (stream: string, n = 1): ChannelEvent => ({
  patches: {
    stream,
    epoch: "e",
    patches: Array.from({ length: n }, (_, i) => ({
      cursor: i,
      body: { remove: { entry_id: "a" } },
    })),
  },
});

function rig(longFrames: LongFrames | null = null) {
  const f = fakeClock();
  /** `bytes`: answer text; `text`: thought and answer text. */
  const state = { commits: 0, bytes: 0, text: 0 };
  const r = new StreamRecorder({
    clock: f.clock,
    reactCommits: () => state.commits,
    longFrames,
    answerBytes: (stream) => (stream === "thread:t1" ? state.bytes : 0),
    transcriptTextBytes: (stream) => (stream === "thread:t1" ? state.text : 0),
  });
  return { f, r, state };
}

/** Stops `r` and gives its sample (for a test that only checks what was recorded so far). */
async function peek(r: StreamRecorder, f: ReturnType<typeof fakeClock>) {
  const stopped = r.stop();
  f.frame();
  return stopped;
}

describe("StreamRecorder", () => {
  test("records each frame's timestamp only while recording", async () => {
    const { f, r } = rig();
    f.frame();
    r.start();
    f.frame(16);
    f.frame(20);
    const sample = await r.stop();
    f.frame();
    expect(sample.raf_timestamps_ms).toEqual([32, 52]);
    expect(f.waiting()).toBe(0);
  });

  test("React commits per frame: mean and max over frames that had a commit", async () => {
    const { f, r, state } = rig();
    state.commits = 7; // before the recording: not counted
    r.start();
    f.frame();
    state.commits += 1;
    f.frame();
    f.frame(); // no commit
    state.commits += 2;
    f.frame();
    const sample = await r.stop();
    expect(sample.react_commits_per_frame).toBe(1.5);
    expect(sample.react_commits_per_frame_max).toBe(2);
  });

  test("no commit while recording reads as null, not 0", async () => {
    const { f, r } = rig();
    r.start();
    f.frame();
    const sample = await r.stop();
    expect(sample.react_commits_per_frame).toBeNull();
    expect(sample.react_commits_per_frame_max).toBeNull();
  });

  test("chunk to paint: a thread's timed patches, from received_ms to just after the frame showing them rendered", async () => {
    const { f, r } = rig();
    r.start();
    const origin = f.clock.timeOrigin;
    f.advance(100);
    r.onTimed(patches("thread:t1"), origin + 95);
    r.onTimed(patches("matter:m"), origin + 95); // not a transcript
    r.onTimed(patches("thread:t1", 0), origin + 95); // changes nothing
    f.frame(5, 3); // 105: the frame that flushes it; its rendering ends at 108, when the posted task runs
    f.frame(16); // 124: the next frame's callback, the double-rAF stamp
    const sample = await r.stop();
    expect(sample.chunk_to_paint_ms).toEqual([13]);
    expect(sample.chunk_to_paint_double_raf_ms).toEqual([29]);
  });

  test("stop waits for samples still waiting for their paint", async () => {
    const { f, r } = rig();
    r.start();
    r.onTimed(patches("thread:t1"), f.clock.timeOrigin);
    const stopped = r.stop();
    f.frame(10, 2);
    f.frame(10);
    const sample = await stopped;
    expect(sample.chunk_to_paint_ms).toEqual([12]);
    expect(sample.chunk_to_paint_double_raf_ms).toEqual([22]);
  });

  test("timed events while not recording are not sampled", async () => {
    const { f, r } = rig();
    r.onTimed(patches("thread:t1"), f.clock.timeOrigin);
    f.frame();
    f.frame();
    r.start();
    expect((await r.stop()).chunk_to_paint_ms).toEqual([]);
  });

  test("first token: no sample while the turn has no agent text yet", async () => {
    const { f, r, state } = rig();
    state.text = 40; // an earlier turn's text
    r.start();
    f.advance(1000);
    r.onPrompt("t1");
    f.advance(30);
    r.onStoreChange(); // the user's message: no agent text yet
    f.tasks();
    expect((await peek(r, f)).first_token_on_screen_ms).toBeNull();
  });

  test("first token closes at the posted task after the flush that grew a thought, not at the first answer", async () => {
    const { f, r, state } = rig();
    r.start();
    f.advance(1000);
    r.onPrompt("t1");
    f.advance(40);
    state.text = 16; // the first thought chunk, applied in this frame's flush
    r.onStoreChange();
    r.onStoreChange(); // a second flush before the paint changes nothing
    f.advance(6); // the frame's rendering
    f.tasks();
    state.text = 400;
    state.bytes = 300;
    r.onStoreChange();
    f.tasks();
    const sample = await r.stop();
    expect(sample.first_token_on_screen_ms).toBe(46);
    expect(sample.answer_text_bytes).toBe(300);
  });

  test("only the first prompt after start counts, and no prompt means no first token and no answer bytes", async () => {
    const { f, r, state } = rig();
    r.start();
    state.text = 10;
    r.onStoreChange();
    f.frame();
    let sample = await r.stop();
    expect(sample.first_token_on_screen_ms).toBeNull();
    expect(sample.answer_text_bytes).toBe(0);

    r.start();
    r.onPrompt("t1");
    f.advance(5);
    r.onPrompt("t2");
    state.text = 20;
    r.onStoreChange();
    f.advance(5);
    f.tasks();
    sample = await r.stop();
    expect(sample.first_token_on_screen_ms).toBe(10);
  });

  test("long animation frames: null without a source, the source's durations with one", async () => {
    expect((await rig().r.stop()).long_animation_frames_ms).toBeNull();
    const calls: string[] = [];
    const { r } = rig({
      start: () => void calls.push("start"),
      takeDurations: () => (calls.push("take"), [51, 80]),
    });
    r.start();
    expect((await r.stop()).long_animation_frames_ms).toEqual([51, 80]);
    expect(calls).toEqual(["start", "take"]);
  });
});
