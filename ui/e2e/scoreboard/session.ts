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
 * before the first answer chunk), React commits per frame and IPC bytes per byte of text (a slower
 * stream batches fewer chunks per event, so both count fewer chunks per frame or per event). The
 * frames (rAF timestamps, so dropped frames, and long animation frames) and chunk to paint stay:
 * each frame and each chunk is measured on its own, however far apart the chunks arrive, and a
 * slower stream only gives fewer of them. Start-up, memory and keystrokes stay too.
 */
export function withoutPacedMetrics(run: RawRun): RawRun {
  return {
    ...run,
    first_token_on_screen_ms: null,
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

/**
 * Where a Windows launch spends its time before `interactive`, each as ms since t0 (the desktop
 * crate's `run()`, the page's `__CLAUSEO_PERF__.t0EpochMs`): to tell a slow process start from a
 * slow webview or a slow page (the GitHub runner reads 1,421 to 1,969 ms where the VM reads 420).
 */
export interface StartupBreakdown {
  /** The spec's spawn of the app: negative, since t0 comes after it. */
  spawn_ms: number;
  /** The spec's CDP connection to the webview. */
  cdp_connected_ms: number;
  /** Spawn to CDP connection. */
  launch_to_cdp_ms: number;
  /** `performance.timeOrigin`: when the webview began navigating to the page. */
  navigation_start_ms: number;
  /** The navigation entry's responseEnd, domContentLoadedEventEnd and loadEventEnd; null when the
   * page has no navigation entry or has not reached the event yet. */
  response_end_ms: number | null;
  dom_content_loaded_ms: number | null;
  load_event_end_ms: number | null;
  /** The perf API's marks (`marks()`), already ms since t0. */
  first_paint_ms: number | null;
  interactive_ms: number | null;
}

/** {@link StartupBreakdown} from the epoch times and the page's own clock. */
export function startupBreakdown(input: {
  t0EpochMs: number;
  /** The page's `performance.timeOrigin`. */
  timeOriginEpochMs: number;
  /** The page's navigation entry: ms on its performance clock, 0 for an event not yet reached. */
  navigation: {
    responseEnd: number;
    domContentLoadedEventEnd: number;
    loadEventEnd: number;
  } | null;
  marks: { first_paint_ms: number | null; interactive_ms: number | null };
  spawnEpochMs: number;
  connectedEpochMs: number;
}): StartupBreakdown {
  const origin = input.timeOriginEpochMs - input.t0EpochMs;
  const nav = (v: number | undefined): number | null =>
    v === undefined || v <= 0 ? null : origin + v;
  return {
    spawn_ms: input.spawnEpochMs - input.t0EpochMs,
    cdp_connected_ms: input.connectedEpochMs - input.t0EpochMs,
    launch_to_cdp_ms: input.connectedEpochMs - input.spawnEpochMs,
    navigation_start_ms: origin,
    response_end_ms: nav(input.navigation?.responseEnd),
    dom_content_loaded_ms: nav(input.navigation?.domContentLoadedEventEnd),
    load_event_end_ms: nav(input.navigation?.loadEventEnd),
    first_paint_ms: input.marks.first_paint_ms,
    interactive_ms: input.marks.interactive_ms,
  };
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
  /** Median chunk to paint as the scoreboard measures it, and as the older double rAF measured
   * the same chunks (`chunk_to_paint_double_raf_ms`), for comparison; null without samples. */
  chunk_to_paint_median_ms: number | null;
  chunk_to_paint_double_raf_median_ms: number | null;
  /** Keystrokes sampled while typing the prompt, and the share of them timed by Event Timing
   * entries rather than the fallbacks (`entry_share`); null where the page gives none. */
  keystroke_count: number | null;
  keystroke_entry_share: number | null;
  startup: StartupBreakdown;
}

/** {@link PlayDebug} from the channel byte counts and the stream sample's lists. */
export function playDebug(input: {
  bytesBefore: number;
  bytesAfter: number;
  answerTextBytes: number;
  raf: readonly number[];
  chunks: readonly number[];
  /** Absent while the page predates the double-rAF list. */
  doubleRafChunks?: readonly number[] | null;
  keystrokeLatency: readonly number[] | null;
  entryShare?: number | null;
  startup: StartupBreakdown;
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
    chunk_to_paint_median_ms: median(input.chunks),
    chunk_to_paint_double_raf_median_ms: median(input.doubleRafChunks ?? []),
    keystroke_count: input.keystrokeLatency?.length ?? null,
    keystroke_entry_share: input.entryShare ?? null,
    startup: input.startup,
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
