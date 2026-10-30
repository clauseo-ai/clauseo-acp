// The speed scoreboard's numbers (R13): what one play of the scoreboard session records, how three
// measured runs become the scoreboard-<os>.json result, and how that result is read back. The
// Windows spec, the macOS `marks` build (through cli.ts) and the comment workflow (comment.ts) all
// use this file, so the definitions live in one place.
//
// It imports nothing but other files of this folder, so plain Node (24 or later, which strips the
// types) runs it without an install: the comment workflow does exactly that.

/** The engines the scoreboard measures: Windows x64 (WebView2) and macOS (WebKit). */
export const OSES = ["windows", "macos"] as const;
export type Os = (typeof OSES)[number];

/** Runs played before the measured ones and thrown away: the first start of a new exe is slow. */
export const WARMUP_RUNS = 1;
/** Measured runs; the result holds the median of each metric over them. */
export const MEASURED_RUNS = 3;
/** A `requestAnimationFrame` gap over this many milliseconds is a dropped frame. */
export const FRAME_BUDGET_MS = 16.7;
/** Dropped frames are counted per this many milliseconds of streaming. */
export const DROPPED_WINDOW_MS = 10_000;
/** A Long Animation Frame entry over this many milliseconds counts (the budget row's 50 ms). */
export const LONG_FRAME_MS = 50;
/** The result format's version. A reader refuses any other. */
export const SCHEMA = 1;

/** Every metric the scoreboard reports, in the comment's order. Each `id` is a budgets.toml row. */
export const METRICS = [
  { id: "warm_start_interactive", label: "Start-up to interactive", unit: "ms" },
  { id: "first_token_on_screen", label: "Send to first token on screen", unit: "ms" },
  { id: "chunk_to_paint_p50", label: "Chunk to paint, p50", unit: "ms" },
  { id: "chunk_to_paint_p95", label: "Chunk to paint, p95", unit: "ms" },
  { id: "long_frames_streaming", label: "Long animation frames while streaming", unit: "count" },
  {
    id: "dropped_frames_streaming",
    label: "Dropped frames while streaming, per 10 s",
    unit: "count",
  },
  { id: "keystroke_latency_p95", label: "Typing to screen, p95", unit: "ms" },
  { id: "keystroke_handler", label: "Keystroke handler, p95", unit: "ms" },
  { id: "idle_memory_total", label: "Memory at idle", unit: "MB" },
  { id: "memory_after_session", label: "Memory after the session", unit: "MB" },
] as const;

export type MetricId = (typeof METRICS)[number]["id"];
export type Unit = (typeof METRICS)[number]["unit"];

/**
 * What one play of the session records, raw. `null` means the engine cannot measure it: WebKit has
 * no Long Animation Frames and no Event Timing, so macOS sends `null` for those three (research 15,
 * risk 4). Every key is required, so a misspelt key fails instead of reading as "not measured".
 */
export interface RawRun {
  /** Process start to the `interactive` mark. */
  start_to_interactive_ms: number | null;
  /** The send click to the first answer token painted. */
  first_token_on_screen_ms: number | null;
  /** Per patch: the bridge's receive time to the frame that painted it (double rAF). */
  chunk_to_paint_ms: number[] | null;
  /** `requestAnimationFrame` timestamps while the session streams and the transcript scrolls. */
  raf_timestamps_ms: number[] | null;
  /** Durations of the `long-animation-frame` entries while streaming. */
  long_animation_frames_ms: number[] | null;
  /** Event Timing `duration` of each composer keystroke. */
  keystroke_latency_ms: number[] | null;
  /** Event Timing `processingEnd - processingStart` of each composer keystroke. */
  keystroke_handler_ms: number[] | null;
  /** Memory of the whole process group before the session (Windows: private working set). */
  idle_memory_mb: number | null;
  /** The same, after the session ended. */
  memory_after_session_mb: number | null;
}

const RAW_KEYS = [
  "start_to_interactive_ms",
  "first_token_on_screen_ms",
  "chunk_to_paint_ms",
  "raf_timestamps_ms",
  "long_animation_frames_ms",
  "keystroke_latency_ms",
  "keystroke_handler_ms",
  "idle_memory_mb",
  "memory_after_session_mb",
] as const satisfies readonly (keyof RawRun)[];

/** The keys of {@link RawRun} that hold a list of samples; the rest hold one number. */
const LIST_KEYS = [
  "chunk_to_paint_ms",
  "raf_timestamps_ms",
  "long_animation_frames_ms",
  "keystroke_latency_ms",
  "keystroke_handler_ms",
] as const satisfies readonly (keyof RawRun)[];

/** One metric of a result: the median over the measured runs, and each run's own value. */
export interface MetricResult {
  median: number | null;
  runs: (number | null)[];
}

/** The content of scoreboard-<os>.json. */
export interface ScoreboardResult {
  schema: typeof SCHEMA;
  os: Os;
  warmup_runs: number;
  measured_runs: number;
  metrics: Record<MetricId, MetricResult>;
}

/** The median; the mean of the two middle values for an even count; null for none. */
export function median(values: readonly number[]): number | null {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  const upper = sorted[mid]!;
  return sorted.length % 2 === 1 ? upper : (sorted[mid - 1]! + upper) / 2;
}

/** The nearest-rank percentile: the smallest value with at least `p`% of values at or below it. */
export function percentile(values: readonly number[], p: number): number | null {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const rank = Math.max(1, Math.ceil((p / 100) * sorted.length));
  return sorted[rank - 1]!;
}

/**
 * Gaps between consecutive rAF timestamps over {@link FRAME_BUDGET_MS}, per 10 s of the span the
 * timestamps cover. Null with fewer than two timestamps. Each gap counts once, however long.
 */
export function droppedFramesPer10s(timestamps: readonly number[]): number | null {
  if (timestamps.length < 2) return null;
  let dropped = 0;
  for (let i = 1; i < timestamps.length; i++) {
    const gap = timestamps[i]! - timestamps[i - 1]!;
    if (gap < 0) throw new Error(`raf_timestamps_ms goes backwards at index ${i}`);
    if (gap > FRAME_BUDGET_MS) dropped++;
  }
  const span = timestamps[timestamps.length - 1]! - timestamps[0]!;
  return span > 0 ? (dropped * DROPPED_WINDOW_MS) / span : null;
}

/** One run's value of each metric. */
export function runMetrics(run: RawRun): Record<MetricId, number | null> {
  const of = (values: number[] | null, f: (v: number[]) => number | null) =>
    values === null ? null : f(values);
  return {
    warm_start_interactive: run.start_to_interactive_ms,
    first_token_on_screen: run.first_token_on_screen_ms,
    chunk_to_paint_p50: of(run.chunk_to_paint_ms, (v) => percentile(v, 50)),
    chunk_to_paint_p95: of(run.chunk_to_paint_ms, (v) => percentile(v, 95)),
    long_frames_streaming: of(
      run.long_animation_frames_ms,
      (v) => v.filter((d) => d > LONG_FRAME_MS).length,
    ),
    dropped_frames_streaming: of(run.raf_timestamps_ms, droppedFramesPer10s),
    keystroke_latency_p95: of(run.keystroke_latency_ms, (v) => percentile(v, 95)),
    keystroke_handler: of(run.keystroke_handler_ms, (v) => percentile(v, 95)),
    idle_memory_total: run.idle_memory_mb,
    memory_after_session: run.memory_after_session_mb,
  };
}

/**
 * The result of {@link MEASURED_RUNS} measured runs, played after {@link WARMUP_RUNS} warm-up runs
 * the caller threw away. A metric is measured in every run, or in none (null, shown as "not
 * measured": an engine that cannot measure it). Measured in some runs but not all means a run
 * went wrong, and throws: the median of two runs is not the median of three.
 */
export function summarise(os: Os, runs: readonly RawRun[]): ScoreboardResult {
  if (runs.length !== MEASURED_RUNS) {
    throw new Error(`expected ${MEASURED_RUNS} measured runs, got ${runs.length}`);
  }
  const perRun = runs.map(runMetrics);
  const metrics = {} as Record<MetricId, MetricResult>;
  for (const { id } of METRICS) {
    const values = perRun.map((m) => m[id]);
    const measured = values.filter((v) => v !== null);
    if (measured.length !== 0 && measured.length !== values.length) {
      throw new Error(
        `${id} was measured in ${measured.length} of ${values.length} runs; it must be every run or none`,
      );
    }
    metrics[id] = { median: median(measured), runs: values };
  }
  return { schema: SCHEMA, os, warmup_runs: WARMUP_RUNS, measured_runs: MEASURED_RUNS, metrics };
}

/** A finite number from 0 to 1e9: the only numbers a run or a result may hold. */
function isValue(v: unknown): v is number {
  return typeof v === "number" && Number.isFinite(v) && v >= 0 && v <= 1e9;
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

/** Checks one raw run, as the page or the `marks` build wrote it. Throws on anything off. */
export function parseRawRun(value: unknown): RawRun {
  if (!isRecord(value)) throw new Error("a run must be a JSON object");
  for (const key of Object.keys(value)) {
    if (!(RAW_KEYS as readonly string[]).includes(key)) throw new Error(`unknown key \`${key}\``);
  }
  for (const key of RAW_KEYS) {
    if (!(key in value)) throw new Error(`missing key \`${key}\` (null means not measured)`);
    const v = value[key];
    if (v === null) continue;
    const isList = (LIST_KEYS as readonly string[]).includes(key);
    const ok = isList ? Array.isArray(v) && v.every(isValue) : isValue(v);
    if (!ok)
      throw new Error(`\`${key}\` must be ${isList ? "a list of numbers" : "a number"} or null`);
  }
  return value as unknown as RawRun;
}

/** What the comment needs from a result: each metric's median. */
export type ResultValues = Record<MetricId, number | null>;

/**
 * Reads scoreboard-<os>.json from an artifact, which is untrusted data: a pull request's own code
 * wrote it. Only the medians of known metrics are kept, each a number from 0 to 1e9 or null; a
 * metric left out reads as null. A metric whose `runs` mix null and numbers, or whose median is
 * null while its runs measured it, is refused, as {@link summarise} would have refused it. Anything
 * else throws, with a message that never quotes the input.
 */
export function parseResult(text: string, os: Os): ResultValues {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    throw new Error("not valid JSON");
  }
  if (!isRecord(value)) throw new Error("not a JSON object");
  if (value["schema"] !== SCHEMA) throw new Error(`schema is not ${SCHEMA}`);
  if (value["os"] !== os) throw new Error(`os is not "${os}"`);
  const metrics = value["metrics"];
  if (!isRecord(metrics)) throw new Error("metrics is not an object");
  const out = {} as ResultValues;
  for (const { id } of METRICS) {
    const entry = Object.hasOwn(metrics, id) ? metrics[id] : undefined;
    if (entry === undefined) {
      out[id] = null;
      continue;
    }
    if (!isRecord(entry)) throw new Error(`metrics.${id} is not an object`);
    const m = entry["median"];
    if (m !== null && !isValue(m))
      throw new Error(`metrics.${id}.median is not a number from 0 to 1e9 or null`);
    const runs = entry["runs"];
    if (runs !== undefined) {
      if (
        !Array.isArray(runs) ||
        runs.length > 16 ||
        !runs.every((r) => r === null || isValue(r))
      ) {
        throw new Error(`metrics.${id}.runs is not a short list of numbers and nulls`);
      }
      const measured = runs.filter((r) => r !== null).length;
      if ((measured !== 0 && measured !== runs.length) || (m === null && measured !== 0)) {
        throw new Error(`metrics.${id} was measured in some runs but not all`);
      }
    }
    out[id] = m;
  }
  return out;
}
