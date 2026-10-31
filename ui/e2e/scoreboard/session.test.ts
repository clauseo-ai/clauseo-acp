// session.ts: the play file names, the Windows spec's per-play debug numbers and the pace check.

import { expect, test } from "@playwright/test";

import type { RawRun } from "./metrics.ts";
import { heldPace, playDebug, playName, withoutPacedMetrics } from "./session.ts";

test("plays are named by number, the warm-up marked", () => {
  expect([1, 2, 3, 4].map(playName)).toEqual(["play-1-warmup", "play-2", "play-3", "play-4"]);
});

test("playDebug: the channel bytes, the rAF count, span and gaps, the chunk samples", () => {
  expect(
    playDebug({
      bytesBefore: 1_000,
      bytesAfter: 431_000,
      answerTextBytes: 80_000,
      raf: [100, 116.7, 133.3, 166.7],
      chunks: [4, 5, 6],
    }),
  ).toEqual({
    channel_bytes_before: 1_000,
    channel_bytes_after: 431_000,
    channel_bytes_session: 430_000,
    answer_text_bytes: 80_000,
    raf_count: 4,
    raf_span_ms: expect.closeTo(66.7, 5),
    raf_gap_median_ms: expect.closeTo(16.7, 5),
    raf_gap_max_ms: expect.closeTo(33.4, 5),
    chunk_samples: 3,
  });
  const empty = playDebug({
    bytesBefore: 0,
    bytesAfter: 0,
    answerTextBytes: 0,
    raf: [5],
    chunks: [],
  });
  expect(empty.raf_span_ms).toBeNull();
  expect(empty.raf_gap_median_ms).toBeNull();
  expect(empty.raf_gap_max_ms).toBeNull();
});

const run = (raf: number[] | null): RawRun => ({
  start_to_interactive_ms: 1_400,
  first_token_on_screen_ms: 390,
  chunk_to_paint_ms: [20, 25],
  raf_timestamps_ms: raf,
  long_animation_frames_ms: [],
  keystroke_latency_ms: [16],
  keystroke_handler_ms: [1],
  idle_memory_mb: 70,
  memory_after_session_mb: 105,
  react_commits_per_frame: 1.9,
  ipc_bytes_per_text_byte: 5,
});

test("heldPace: a stream up to twice the paced 13 s held it; a longer one did not", () => {
  expect(heldPace(run([0, 15_000]))).toBe(true);
  expect(heldPace(run([0, 26_000]))).toBe(true);
  expect(heldPace(run([0, 85_000]))).toBe(false);
  expect(heldPace(run(null))).toBeNull();
  expect(heldPace(run([5]))).toBeNull();
});

test("withoutPacedMetrics keeps start-up, memory and keystrokes and drops the paced numbers", () => {
  expect(withoutPacedMetrics(run([0, 85_000]))).toEqual({
    ...run(null),
    first_token_on_screen_ms: null,
    chunk_to_paint_ms: null,
    long_animation_frames_ms: null,
    react_commits_per_frame: null,
    ipc_bytes_per_text_byte: null,
  });
});
