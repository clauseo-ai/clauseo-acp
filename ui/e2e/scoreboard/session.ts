// What both scoreboard runners play: fake_agent's bundled `scoreboard` scenario at a fixed pace,
// and how a result prints on a terminal. The Windows spec (app-windows/scoreboard.spec.ts) and the
// macOS runner (macos.ts) import this; like metrics.ts it imports only this folder, so plain Node
// runs it.

import { METRICS, type ScoreboardResult } from "./metrics.ts";
import { formatNumber } from "./render.ts";

/** fake_agent's bundled scenario (crates/fake_agent/scenarios/scoreboard.jsonl). */
export const SCENARIO = "scoreboard";
/** fake_agent's arguments after `--scenario <name>`: one step every 5 ms, as the scenario says. */
export const PACE_ARGS: readonly string[] = ["--pace-ms", "5"];
/** The prompt the Windows spec types; the scenario accepts any prompt text. */
export const PROMPT = "Review the MSA and mark up clause 4";

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
