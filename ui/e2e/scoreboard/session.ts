// What both scoreboard runners play: fake_agent's bundled `scoreboard` scenario at a fixed pace,
// and how a result prints on a terminal. The Windows spec (app-windows/scoreboard.spec.ts) and the
// macOS runner (macos.ts) import this; like metrics.ts it imports only this folder, so plain Node
// runs it.

import { METRICS, WARMUP_RUNS, median, type RawRun, type ScoreboardResult } from "./metrics.ts";
import { formatNumber } from "./render.ts";

/** fake_agent's bundled scenario (crates/fake_agent/scenarios/scoreboard.jsonl). */
export const SCENARIO = "scoreboard";
/** fake_agent's arguments after `--scenario <name>`: one step every 5 ms, as the scenario says. */
export const PACE_ARGS: readonly string[] = ["--pace-ms", "5"];
/**
 * How long the stream takes when every step keeps the 5 ms pace: about 13 s (the scenario's
 * README; Windows records rAF over about 15 s, typing and scrolling included).
 */
export const PACED_STREAM_MS = 13_000;
/** A stream whose rAF recording spans more than this many times {@link PACED_STREAM_MS} did not
 * hold the pace: the machine stretched the agent's sleeps. */
export const PACE_SLACK = 2;

/**
 * Whether a run's stream held the pace, judged by the span of its rAF timestamps; null when the
 * run has no rAF timestamps. GitHub's macos-15 runner did not: 200 sleeps of 5 ms took 5.5 to
 * 6.2 s there, so the stream ran about 85 s instead of 13 s (PR #106's runs).
 */
export function heldPace(run: RawRun): boolean | null {
  const raf = run.raf_timestamps_ms;
  if (raf === null || raf.length < 2) return null;
  return raf[raf.length - 1]! - raf[0]! <= PACED_STREAM_MS * PACE_SLACK;
}

/**
 * The run without the numbers a stretched pace distorts: first token (it waits out the paced steps
 * before the first answer chunk), chunk to paint, dropped frames, React commits per frame and IPC
 * bytes per byte of text (a slower stream batches fewer chunks per event). Start-up, memory and
 * keystrokes do not depend on the pace and stay.
 */
export function withoutPacedMetrics(run: RawRun): RawRun {
  return {
    ...run,
    first_token_on_screen_ms: null,
    chunk_to_paint_ms: null,
    raf_timestamps_ms: null,
    long_animation_frames_ms: null,
    react_commits_per_frame: null,
    ipc_bytes_per_text_byte: null,
  };
}

/** The prompt the Windows spec types; the scenario accepts any prompt text. */
export const PROMPT = "Review the MSA and mark up clause 4";

/**
 * The folder under SCOREBOARD_OUT that holds each play's raw run (and, on Windows, its debug
 * numbers), for a person to check the medians against. Uploaded as its own artifact
 * (`scoreboard-<os>-runs`), which the comment never reads.
 */
export const RUNS_DIR = "runs";

/** The file stem of play `n` (1-based): `play-1-warmup`, then `play-2` and on. */
export function playName(n: number): string {
  return n <= WARMUP_RUNS ? `play-${n}-warmup` : `play-${n}`;
}

/** What the Windows spec writes beside each play's raw run, to check the medians by hand. */
export interface PlayDebug {
  channel_bytes_before: number;
  channel_bytes_after: number;
  channel_bytes_session: number;
  answer_text_bytes: number;
  raf_count: number;
  /** Last rAF timestamp minus the first; null with fewer than two. */
  raf_span_ms: number | null;
  /** Median and largest gap between consecutive rAF timestamps: the frame interval and the worst. */
  raf_gap_median_ms: number | null;
  raf_gap_max_ms: number | null;
  /** Chunk-to-paint samples: timed events that patched the transcript while recording. */
  chunk_samples: number;
}

/** {@link PlayDebug} from the channel byte counts and the stream sample's lists. */
export function playDebug(input: {
  bytesBefore: number;
  bytesAfter: number;
  answerTextBytes: number;
  raf: readonly number[];
  chunks: readonly number[];
}): PlayDebug {
  const { raf } = input;
  const gaps = raf.slice(1).map((t, i) => t - raf[i]!);
  return {
    channel_bytes_before: input.bytesBefore,
    channel_bytes_after: input.bytesAfter,
    channel_bytes_session: input.bytesAfter - input.bytesBefore,
    answer_text_bytes: input.answerTextBytes,
    raf_count: raf.length,
    raf_span_ms: raf.length < 2 ? null : raf[raf.length - 1]! - raf[0]!,
    raf_gap_median_ms: median(gaps),
    raf_gap_max_ms: gaps.length === 0 ? null : Math.max(...gaps),
    chunk_samples: input.chunks.length,
  };
}

/** A result as a plain text table: each metric's median and its three runs. */
export function resultTable(result: ScoreboardResult): string {
  const cell = (v: number | null): string => (v === null ? "not measured" : formatNumber(v));
  const rows = METRICS.map((m) => {
    const r = result.metrics[m.id];
    return [m.label, m.unit, cell(r.median), r.runs.map(cell).join(", ")];
  });
  const head = ["Metric", "Unit", "Median", "Runs"];
  const widths = head.map((h, i) => Math.max(h.length, ...rows.map((row) => row[i]!.length)));
  const line = (row: string[]): string =>
    row
      .map((c, i) => c.padEnd(widths[i]!))
      .join("  ")
      .trimEnd();
  return (
    [
      `Speed scoreboard, ${result.os}: median of ${result.measured_runs} runs after ${result.warmup_runs} warm-up`,
      line(head),
      line(widths.map((w) => "-".repeat(w))),
      ...rows.map(line),
    ].join("\n") + "\n"
  );
}
