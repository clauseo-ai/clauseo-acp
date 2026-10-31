// session.ts: the play file names and the Windows spec's per-play debug numbers.

import { expect, test } from "@playwright/test";

import { playDebug, playName } from "./session.ts";

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
