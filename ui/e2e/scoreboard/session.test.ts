// session.ts: the play file names, the Windows spec's per-play debug numbers and the pace check.

import { expect, test } from "@playwright/test";

import type { RawRun } from "./metrics.ts";
import {
  heldPace,
  playDebug,
  playName,
  startupBreakdown,
  withoutPacedMetrics,
  type StartupBreakdown,
} from "./session.ts";

test("plays are named by number, the warm-up marked", () => {
  expect([1, 2, 3, 4].map(playName)).toEqual(["play-1-warmup", "play-2", "play-3", "play-4"]);
});

test("startupBreakdown: every step as ms since t0, events not reached as null", () => {
  const t0 = 1_700_000_000_000;
  expect(
    startupBreakdown({
      t0EpochMs: t0,
      timeOriginEpochMs: t0 + 300,
      navigation: { responseEnd: 50, domContentLoadedEventEnd: 400, loadEventEnd: 0 },
      marks: { first_paint_ms: 900, interactive_ms: 1_400 },
      spawnEpochMs: t0 - 120,
      connectedEpochMs: t0 + 1_000,
    }),
  ).toEqual({
    spawn_ms: -120,
    cdp_connected_ms: 1_000,
    launch_to_cdp_ms: 1_120,
    navigation_start_ms: 300,
    response_end_ms: 350,
    dom_content_loaded_ms: 700,
    load_event_end_ms: null,
    first_paint_ms: 900,
    interactive_ms: 1_400,
  });
  const none = startupBreakdown({
    t0EpochMs: t0,
    timeOriginEpochMs: t0,
    navigation: null,
    marks: { first_paint_ms: null, interactive_ms: null },
    spawnEpochMs: t0,
    connectedEpochMs: t0,
  });
  expect(none.response_end_ms).toBeNull();
  expect(none.interactive_ms).toBeNull();
});

const startup: StartupBreakdown = {
  spawn_ms: -100,
  cdp_connected_ms: 900,
  launch_to_cdp_ms: 1_000,
  navigation_start_ms: 300,
  response_end_ms: 320,
  dom_content_loaded_ms: 600,
  load_event_end_ms: 650,
  first_paint_ms: 800,
  interactive_ms: 1_200,
};

test("playDebug: the channel bytes, the rAF count, span and gaps, the chunks, keystrokes, start-up", () => {
  expect(
    playDebug({
      bytesBefore: 1_000,
      bytesAfter: 431_000,
      answerTextBytes: 80_000,
      raf: [100, 116.7, 133.3, 166.7],
      chunks: [4, 5, 6],
      doubleRafChunks: [20, 30, 25, 40],
      keystrokeLatency: [10, 12, 14],
      entryShare: 2 / 3,
      startup,
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
    chunk_to_paint_median_ms: 5,
    chunk_to_paint_double_raf_median_ms: 27.5,
    keystroke_count: 3,
    keystroke_entry_share: expect.closeTo(0.667, 3),
    startup,
  });
  // A page without the double-rAF list or entry_share, and with no keystroke samples.
  const empty = playDebug({
    bytesBefore: 0,
    bytesAfter: 0,
    answerTextBytes: 0,
    raf: [5],
    chunks: [],
    keystrokeLatency: null,
    startup,
  });
  expect(empty.chunk_to_paint_median_ms).toBeNull();
  expect(empty.chunk_to_paint_double_raf_median_ms).toBeNull();
  expect(empty.keystroke_count).toBeNull();
  expect(empty.keystroke_entry_share).toBeNull();
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

test("withoutPacedMetrics drops first token, React commits and IPC, and keeps the frames and chunk to paint", () => {
  expect(withoutPacedMetrics(run([0, 85_000]))).toEqual({
    ...run([0, 85_000]),
    first_token_on_screen_ms: null,
    react_commits_per_frame: null,
    ipc_bytes_per_text_byte: null,
  });
});
