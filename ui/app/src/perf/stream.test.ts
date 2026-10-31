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
  const state = { commits: 0, bytes: 0 };
  const r = new StreamRecorder({
    clock: f.clock,
    reactCommits: () => state.commits,
    longFrames,
    answerBytes: (stream) => (stream === "thread:t1" ? state.bytes : 0),
  });
  return { f, r, state };
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

  test("chunk to paint: a thread's timed patches, from received_ms to the second frame after, in epoch ms", async () => {
    const { f, r } = rig();
    r.start();
    const origin = f.clock.timeOrigin;
    f.advance(100);
    r.onTimed(patches("thread:t1"), origin + 95);
    r.onTimed(patches("matter:m"), origin + 95); // not a transcript
    r.onTimed(patches("thread:t1", 0), origin + 95); // changes nothing
    f.frame(5); // 105: the frame that flushes it
    f.frame(16); // 121: that frame was painted
    const sample = await r.stop();
    expect(sample.chunk_to_paint_ms).toEqual([26]);
  });

  test("stop waits for a chunk still waiting for its paint", async () => {
    const { f, r } = rig();
    r.start();
    r.onTimed(patches("thread:t1"), f.clock.timeOrigin);
    const stopped = r.stop();
    f.frame(10);
    f.frame(10);
    expect((await stopped).chunk_to_paint_ms).toEqual([20]);
  });

  test("timed events while not recording are not sampled", async () => {
    const { f, r } = rig();
    r.onTimed(patches("thread:t1"), f.clock.timeOrigin);
    f.frame();
    f.frame();
    r.start();
    expect((await r.stop()).chunk_to_paint_ms).toEqual([]);
  });

  test("first token: the send to the frame after the flush in which the answer text grew", async () => {
    const { f, r, state } = rig();
    state.bytes = 40; // an earlier turn's answer
    r.start();
    f.advance(1000);
    r.onPrompt("t1");
    f.advance(30);
    r.onStoreChange(); // the user's message: no answer text yet
    f.frame(10);
    state.bytes = 45;
    r.onStoreChange();
    r.onStoreChange(); // a second flush before the paint changes nothing
    f.frame(16);
    state.bytes = 400;
    const sample = await r.stop();
    expect(sample.first_token_on_screen_ms).toBe(56);
    expect(sample.answer_text_bytes).toBe(400);
  });

  test("only the first prompt after start counts, and no prompt means no first token and no answer bytes", async () => {
    const { f, r, state } = rig();
    r.start();
    state.bytes = 10;
    r.onStoreChange();
    f.frame();
    let sample = await r.stop();
    expect(sample.first_token_on_screen_ms).toBeNull();
    expect(sample.answer_text_bytes).toBe(0);

    r.start();
    r.onPrompt("t1");
    f.advance(5);
    r.onPrompt("t2");
    state.bytes = 20;
    r.onStoreChange();
    f.frame(5);
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
