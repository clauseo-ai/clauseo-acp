// metrics.ts and inputs.ts on fixed inputs: the statistics, the result format and its checks, and
// budgets.toml as the comment reads it.

import { readFileSync } from "node:fs";

import { expect, test } from "@playwright/test";

import { parseBudgets, parseReducerSummaries } from "./inputs.ts";
import {
  BLANK_APP_IDLE_MB,
  BYTES_PER_MB,
  METRICS,
  RAW_RUN_KEYS,
  isReportOnly,
  droppedFramesPer10s,
  median,
  parseRawRun,
  parseResult,
  percentile,
  summarise,
  type RawRun,
} from "./metrics.ts";

const repoFile = (path: string) =>
  readFileSync(new URL(`../../../${path}`, import.meta.url), "utf8");
const fixture = (name: string) =>
  readFileSync(new URL(`fixtures/${name}`, import.meta.url), "utf8");

/** A Windows run: every metric measured. */
function windowsRun(scale: number): RawRun {
  return {
    start_to_interactive_ms: 700 * scale,
    first_token_on_screen_ms: 90 * scale,
    chunk_to_paint_ms: [4, 5, 6, 7, 8, 9, 10, 11, 12, 40].map((v) => v * scale),
    raf_timestamps_ms: [0, 16, 32, 48, 100, 116, 132, 148, 164, 1000],
    long_animation_frames_ms: [30, 60, 120],
    keystroke_latency_ms: [10, 12, 30].map((v) => v * scale),
    keystroke_handler_ms: [1, 2, 3],
    idle_memory_mb: 180 * scale,
    memory_after_session_mb: 240 * scale,
    react_commits_per_frame: 1.5,
    ipc_bytes_per_text_byte: 2.5 * scale,
  };
}

/** A macOS run: WebKit has no Long Animation Frames and no Event Timing. */
function macosRun(scale: number): RawRun {
  return {
    ...windowsRun(scale),
    long_animation_frames_ms: null,
    keystroke_latency_ms: null,
    keystroke_handler_ms: null,
  };
}

test("median: the middle value, the mean of the two middle ones, or null", () => {
  expect(median([3, 1, 2])).toBe(2);
  expect(median([4, 1, 3, 2])).toBe(2.5);
  expect(median([])).toBeNull();
});

test("percentile: nearest rank", () => {
  const values = Array.from({ length: 100 }, (_, i) => i + 1);
  expect(percentile(values, 50)).toBe(50);
  expect(percentile(values, 95)).toBe(95);
  expect(percentile([7], 95)).toBe(7);
  expect(percentile([1, 2, 3], 50)).toBe(2);
  expect(percentile([], 50)).toBeNull();
});

test("dropped frames: rAF gaps over 16.7 ms, per 10 s of the span", () => {
  // Steady 60 Hz: no gap over 16.7 ms.
  const steady = Array.from({ length: 601 }, (_, i) => i * (1000 / 60));
  expect(droppedFramesPer10s(steady)).toBe(0);
  // Two long gaps (52 ms and 836 ms) in a span of 1 s: 20 per 10 s.
  expect(droppedFramesPer10s([0, 16, 32, 48, 100, 116, 132, 148, 164, 1000])).toBe(20);
  // A gap of exactly 16.7 ms is not dropped; 16.8 is.
  expect(droppedFramesPer10s([0, 16.7])).toBe(0);
  expect(droppedFramesPer10s([0, 16.8])).toBeCloseTo((1 * 10_000) / 16.8);
  expect(droppedFramesPer10s([5])).toBeNull();
  expect(() => droppedFramesPer10s([10, 5])).toThrow("backwards");
});

test("summarise: the median of three measured runs for each metric", () => {
  const result = summarise("windows", [windowsRun(1), windowsRun(1.2), windowsRun(0.9)]);
  expect(result.schema).toBe(1);
  expect(result.warmup_runs).toBe(1);
  expect(result.measured_runs).toBe(3);
  expect(Object.keys(result.metrics)).toEqual(METRICS.map((m) => m.id));
  expect(result.metrics.first_token_on_screen).toEqual({ median: 90, runs: [90, 108, 81] });
  // p50 of ten samples is the 5th (8), p95 the 10th (40), per run; then the median of the runs.
  expect(result.metrics.chunk_to_paint_p50.median).toBe(8);
  expect(result.metrics.chunk_to_paint_p95.median).toBe(40);
  expect(result.metrics.long_frames_streaming.median).toBe(2);
  expect(result.metrics.dropped_frames_streaming.median).toBe(20);
  expect(result.metrics.keystroke_latency_p95.median).toBe(30);
  expect(result.metrics.keystroke_handler.median).toBe(3);
  expect(result.metrics.memory_after_session.median).toBe(240);
  expect(result.metrics.react_commits_per_frame_streaming.median).toBe(1.5);
  expect(result.metrics.ipc_bytes_per_text_byte.median).toBe(2.5);
  expect(result.metrics.idle_memory_over_blank.median).toBeCloseTo(180 - BLANK_APP_IDLE_MB);
  // Not measurable yet: no input method on a runner, no lite mode in the app.
  expect(result.metrics.keystroke_latency_ime_p95).toEqual({
    median: null,
    runs: [null, null, null],
  });
  expect(result.metrics.chunk_to_paint_lite_p50.median).toBeNull();
});

test("the blank app's idle memory: run 41206906369's 72,118,272 B in MiB", () => {
  expect(BYTES_PER_MB).toBe(1_048_576);
  expect(BLANK_APP_IDLE_MB).toBeCloseTo(68.78, 2);
});

test("memory over the blank app: Windows only, never below zero", () => {
  const low = { ...windowsRun(1), idle_memory_mb: 50 };
  const result = summarise("windows", [low, low, low]);
  expect(result.metrics.idle_memory_over_blank.median).toBe(0);
  const noIdle = { ...windowsRun(1), idle_memory_mb: null };
  expect(
    summarise("windows", [noIdle, noIdle, noIdle]).metrics.idle_memory_over_blank.median,
  ).toBeNull();
});

test("summarise: what WebKit cannot measure is null, not zero", () => {
  const result = summarise("macos", [macosRun(1), macosRun(1), macosRun(1)]);
  expect(result.metrics.long_frames_streaming).toEqual({ median: null, runs: [null, null, null] });
  expect(result.metrics.keystroke_latency_p95.median).toBeNull();
  expect(result.metrics.keystroke_handler.median).toBeNull();
  expect(result.metrics.first_token_on_screen.median).toBe(90);
  // The blank app was measured on Windows only.
  expect(result.metrics.idle_memory_over_blank).toEqual({ median: null, runs: [null, null, null] });
  expect(result.metrics.idle_memory_total.median).toBe(180);
});

test("summarise: a metric measured in some runs but not all is refused", () => {
  // The scroll failed in one run: its dropped frames are null while the other two have them.
  const broken = { ...windowsRun(1), raf_timestamps_ms: null };
  expect(() => summarise("windows", [windowsRun(1), broken, windowsRun(1)])).toThrow(
    "dropped_frames_streaming was measured in 2 of 3 runs; it must be every run or none",
  );
});

test("summarise: exactly three measured runs", () => {
  expect(() => summarise("windows", [windowsRun(1), windowsRun(1)])).toThrow(
    "expected 3 measured runs, got 2",
  );
});

test("parseRawRun: every key, nothing else, numbers or null", () => {
  expect(parseRawRun(JSON.parse(JSON.stringify(macosRun(1))))).toEqual(macosRun(1));
  const { idle_memory_mb: _, ...missing } = windowsRun(1);
  expect(() => parseRawRun(missing)).toThrow("missing key `idle_memory_mb`");
  expect(() => parseRawRun({ ...windowsRun(1), first_token_ms: 3 })).toThrow(
    "unknown key `first_token_ms`",
  );
  expect(() => parseRawRun({ ...windowsRun(1), chunk_to_paint_ms: 5 })).toThrow(
    "a list of numbers",
  );
  expect(() => parseRawRun({ ...windowsRun(1), idle_memory_mb: -1 })).toThrow("a number or null");
  expect(() => parseRawRun([])).toThrow("JSON object");
});

test("RAW_RUN_KEYS lists every key of a run, in order", () => {
  expect(RAW_RUN_KEYS).toEqual(Object.keys(windowsRun(1)));
  const { ipc_bytes_per_text_byte: _, ...old } = windowsRun(1);
  expect(() => parseRawRun(old)).toThrow("missing key `ipc_bytes_per_text_byte`");
});

test("parseResult reads a result written before the newer metrics existed", () => {
  // scoreboard-windows-main.json has the first half's ten metrics only, as main's artifacts do.
  const values = parseResult(fixture("scoreboard-windows-main.json"), "windows");
  expect(values.first_token_on_screen).toBe(141);
  expect(values.idle_memory_over_blank).toBeNull();
  expect(values.react_commits_per_frame_streaming).toBeNull();
  expect(values.ipc_bytes_per_text_byte).toBeNull();
});

test("parseResult reads back what summarise wrote", () => {
  const result = summarise("windows", [windowsRun(1), windowsRun(1), windowsRun(1)]);
  const values = parseResult(JSON.stringify(result), "windows");
  expect(values.first_token_on_screen).toBe(90);
  expect(values.keystroke_handler).toBe(3);
});

test("parseResult treats the artifact as untrusted and never quotes it", () => {
  const ok = JSON.parse(fixture("scoreboard-windows.json"));
  const bad = (change: (v: Record<string, unknown>) => unknown) =>
    JSON.stringify(change(structuredClone(ok)));
  expect(() => parseResult("<html>", "windows")).toThrow("not valid JSON");
  expect(() => parseResult("[]", "windows")).toThrow("not a JSON object");
  expect(() =>
    parseResult(
      bad((v) => ({ ...v, schema: 2 })),
      "windows",
    ),
  ).toThrow("schema is not 1");
  expect(() => parseResult(JSON.stringify(ok), "macos")).toThrow('os is not "macos"');
  const evil = "<img src=x onerror=alert(1)>";
  let message = "";
  try {
    parseResult(
      bad((v) => {
        (v["metrics"] as Record<string, { median: unknown }>)["first_token_on_screen"]!.median =
          evil;
        return v;
      }),
      "windows",
    );
  } catch (error) {
    message = (error as Error).message;
  }
  expect(message).toBe(
    "metrics.first_token_on_screen.median is not a number from 0 to 1e9 or null",
  );
  expect(() =>
    parseResult(
      bad((v) => {
        (v["metrics"] as Record<string, { median: unknown }>)["idle_memory_total"]!.median = 1e12;
        return v;
      }),
      "windows",
    ),
  ).toThrow("idle_memory_total");
  // Unknown metrics are ignored; a known one left out reads as not measured.
  const values = parseResult(
    bad((v) => {
      const metrics = v["metrics"] as Record<string, unknown>;
      delete metrics["memory_after_session"];
      metrics["__proto__x"] = { median: 1 };
      return v;
    }),
    "windows",
  );
  expect(values.memory_after_session).toBeNull();
  expect(Object.keys(values)).toEqual(METRICS.map((m) => m.id));
  // A metric measured in some runs but not all, or a null median over measured runs, is refused.
  const runs = (r: unknown[], median: number | null = 90) =>
    bad((v) => {
      (v["metrics"] as Record<string, unknown>)["first_token_on_screen"] = { median, runs: r };
      return v;
    });
  expect(() => parseResult(runs([90, null, 91]), "windows")).toThrow(
    "metrics.first_token_on_screen was measured in some runs but not all",
  );
  expect(() => parseResult(runs([90, 90, 91], null), "windows")).toThrow("some runs but not all");
  expect(() => parseResult(runs(["x"]), "windows")).toThrow("runs is not a short list");
  expect(parseResult(runs([null, null, null], null), "windows").first_token_on_screen).toBeNull();
});

test("budgets.toml has a row for every metric but the report-only ones, in the same unit", () => {
  const budgets = parseBudgets(repoFile("budgets.toml"));
  for (const metric of METRICS) {
    if (isReportOnly(metric)) {
      // A row added later would be silently ignored: drop the flag instead.
      expect(budgets.get(metric.id), metric.id).toBeUndefined();
      expect(budgets.get(`${metric.id}_vm`), metric.id).toBeUndefined();
      continue;
    }
    expect(budgets.get(metric.id), metric.id).toBeDefined();
    expect(budgets.get(metric.id)!.unit, metric.id).toBe(metric.unit);
    const vm = budgets.get(`${metric.id}_vm`);
    if (vm !== undefined) expect(vm.unit, `${metric.id}_vm`).toBe(metric.unit);
  }
  expect(METRICS.filter(isReportOnly).map((m) => m.id)).toEqual([
    "react_commits_per_frame_streaming",
    "ipc_bytes_per_text_byte",
  ]);
  // The VM limits decided in #120.
  expect(budgets.get("dropped_frames_streaming_vm")!.limit).toBe(100);
  expect(budgets.get("long_frames_streaming_vm")!.limit).toBe(8);
  // R13's new rows, as proposed in the brief.
  expect(budgets.get("first_token_on_screen")!.limit).toBe(150);
  expect(budgets.get("chunk_to_paint_p50")!.limit).toBe(17);
  expect(budgets.get("chunk_to_paint_p95")!.limit).toBe(50);
  expect(budgets.get("dropped_frames_streaming")!.limit).toBe(30);
  expect(budgets.get("memory_after_session")!.limit).toBe(300);
});

test("parseBudgets reads the flat rows, comments and all", () => {
  const text = [
    "# a comment",
    "[[budget]]",
    'id = "a"',
    "limit = 1_500.0 # proposed",
    'unit = "ms"',
    "",
    "[[budget]]",
    'id = "b"',
    'unit = "MB"',
  ].join("\n");
  expect([...parseBudgets(text)]).toEqual([["a", { limit: 1500, unit: "ms" }]]);
});

test("parseReducerSummaries: each case's name and total Ir", () => {
  const counts = parseReducerSummaries(fixture("reducer-bench-summary.json"));
  expect([...counts]).toEqual([
    ["apply_group::fold_events.scripted_turn", 1_234_567],
    ["apply_group::one_event_on_long_thread.text_on_8k", 41_000],
  ]);
});

test("parseReducerSummaries reads what ci.yml's collect step made of a real reducer-bench run", () => {
  // Run 41120092360's nine gungraun summaries (137,670 bytes) through the step's jq: 1,184 bytes.
  const counts = parseReducerSummaries(fixture("reducer-bench-summary-run-41120092360.json"));
  expect(counts.size).toBe(9);
  expect(counts.get("apply_group::fold_events.replay_20k")).toBe(46_602_348);
});

test("parseReducerSummaries drops a case whose name could inject Markdown", () => {
  const value = JSON.parse(fixture("reducer-bench-summary.json")) as {
    cases: Record<string, unknown>[];
  };
  value.cases[0]!["module_path"] = "apply::group::x](https://evil.example)";
  value.cases[1]!["id"] = "a|b";
  expect(parseReducerSummaries(JSON.stringify(value)).size).toBe(0);
  value.cases[0]!["module_path"] = "apply::group::x";
  value.cases[0]!["ir"] = -1;
  expect(parseReducerSummaries(JSON.stringify(value)).size).toBe(0);
  expect(() => parseReducerSummaries("[]")).toThrow("not a JSON object");
  expect(() => parseReducerSummaries('{"schema":2,"cases":[]}')).toThrow("schema is not 1");
  expect(() => parseReducerSummaries('{"schema":1}')).toThrow("cases is not a list");
  expect(() =>
    parseReducerSummaries(JSON.stringify({ schema: 1, cases: new Array(65).fill({}) })),
  ).toThrow("more than 64 cases");
});
