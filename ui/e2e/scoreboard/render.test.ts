// render.ts: the flags, the change column and the comment body for found, missing, pending and
// refused numbers.

import { readFileSync } from "node:fs";

import { expect, test } from "@playwright/test";

import { parseBudgets, parseReducerSummaries } from "./inputs.ts";
import { parseResult } from "./metrics.ts";
import { MARKER, change, flags, formatNumber, renderComment, type RenderInput } from "./render.ts";

const fixture = (name: string) =>
  readFileSync(new URL(`fixtures/${name}`, import.meta.url), "utf8");
const budgets = parseBudgets(
  readFileSync(new URL("../../../budgets.toml", import.meta.url), "utf8"),
);
const HEAD = "a".repeat(40);
const MAIN = "b".repeat(40);
const run = (id: number) => `https://github.com/clauseo/clauseo-acp/actions/runs/${id}`;

function input(): RenderInput {
  return {
    headSha: HEAD,
    budgets,
    scoreboard: {
      windows: {
        pr: {
          state: "found",
          data: parseResult(fixture("scoreboard-windows.json"), "windows"),
          url: run(1),
          sha: HEAD,
        },
        main: {
          state: "found",
          data: parseResult(fixture("scoreboard-windows-main.json"), "windows"),
          url: run(2),
          sha: MAIN,
        },
      },
      macos: { pr: { state: "not-run" }, main: { state: "not-run" } },
    },
    reducer: {
      pr: {
        state: "found",
        data: parseReducerSummaries(fixture("reducer-bench-summary.json")),
        url: run(3),
        sha: HEAD,
      },
      main: {
        state: "found",
        data: parseReducerSummaries(fixture("reducer-bench-summary-main.json")),
        url: run(4),
        sha: MAIN,
      },
    },
  };
}

/** The table row whose first cell is `label`. */
function row(body: string, label: string): string {
  const line = body.split("\n").find((l) => l.startsWith(`| ${label} |`));
  if (line === undefined) throw new Error(`no row ${label} in:\n${body}`);
  return line;
}

/** Every row of the section under `heading`, up to the next heading. */
function section(body: string, heading: string): string[] {
  const start = body.indexOf(heading);
  const rest = body.slice(start + heading.length);
  const end = rest.indexOf("\n### ");
  return (end === -1 ? rest : rest.slice(0, end))
    .split("\n")
    .filter((l) => l.startsWith("| ") && !l.startsWith("| Metric"));
}

test("flags: over budget, more than 10% worse than main, both or none", () => {
  expect(flags(151, 140, 150)).toEqual(["over budget"]);
  expect(flags(150, 140, 150)).toEqual([]);
  expect(flags(111, 100, null)).toEqual(["more than 10% worse than main"]);
  expect(flags(110, 100, null)).toEqual([]);
  expect(flags(200, 100, 150)).toEqual(["over budget", "more than 10% worse than main"]);
  // Main at zero: any rise is more than 10% worse.
  expect(flags(1, 0, null)).toEqual(["more than 10% worse than main"]);
  expect(flags(null, 100, 150)).toEqual([]);
  expect(flags(90, null, 150)).toEqual([]);
});

test("change: the difference, with the percentage when main is above zero", () => {
  expect(change(162, 141, "ms")).toBe("+21 ms (+14.9%)");
  expect(change(9.4, 9.5, "ms")).toBe("−0.1 ms (−1.1%)");
  expect(change(3, 0, "count")).toBe("+3");
  expect(change(4.2, 4.4, "count")).toBe("−0.2 (−4.5%)");
  expect(change(5, 5, "ms")).toBe("0");
  expect(change(null, 5, "ms")).toBe("–");
  expect(formatNumber(1_234_567)).toBe("1,234,567");
  expect(formatNumber(44.25)).toBe("44.3");
});

test("the comment starts with the marker and flags the rows the brief names", () => {
  const body = renderComment(input());
  expect(body.startsWith(`${MARKER}\n`)).toBe(true);
  expect(row(body, "Send to first token on screen")).toBe(
    "| Send to first token on screen | 162 ms | 141 ms | +21 ms (+14.9%) | 150 ms | ⚠️ over budget, more than 10% worse than main |",
  );
  expect(row(body, "Chunk to paint, p95")).toContain("⚠️ more than 10% worse than main");
  expect(row(body, "Chunk to paint, p50")).toBe(
    "| Chunk to paint, p50 | 9.5 ms | 9.4 ms | +0.1 ms (+1.1%) | 17 ms |  |",
  );
  expect(row(body, "Memory after the session")).toContain("| 300 MB |");
  expect(body).toContain(`main from [its last successful run](${run(2)}) at \`bbbbbbb\``);
});

test("a job that did not run shows every metric as not run", () => {
  const macos = section(renderComment(input()), "### macOS");
  expect(macos).toHaveLength(10);
  for (const line of macos)
    expect(line).toMatch(/^\| [^|]+ \| not run \| not run \| – \| [^|]+ \|  \|$/);
});

test("what an engine cannot measure shows as not measured", () => {
  const value = input();
  value.scoreboard.macos.pr = {
    state: "found",
    data: parseResult(fixture("scoreboard-macos.json"), "macos"),
    url: run(5),
    sha: HEAD,
  };
  const macos = section(renderComment(value), "### macOS");
  expect(macos.find((l) => l.startsWith("| Long animation frames"))).toContain(
    "| not measured | not run | – |",
  );
  expect(macos.find((l) => l.startsWith("| Typing to screen"))).toContain("| not measured |");
  expect(macos.find((l) => l.startsWith("| Send to first token"))).toContain(
    "| 88 ms | not run | – | 150 ms |",
  );
});

test("pending and refused results say so", () => {
  const value = input();
  value.scoreboard.windows.pr = { state: "pending", url: run(6) };
  value.scoreboard.macos.pr = { state: "invalid", url: run(7), reason: "schema is not 1" };
  const body = renderComment(value);
  expect(section(body, "### Windows")[0]).toContain("| pending | 760 ms |");
  expect(section(body, "### macOS")[0]).toContain("| invalid result |");
  expect(body).toContain(`this PR's [scoreboard result](${run(7)}) was refused: schema is not 1`);
});

test("reducer counts: one row per case, flagged over 10% worse; or not run", () => {
  const body = renderComment(input());
  expect(row(body, "`apply_group::fold_events.scripted_turn`")).toBe(
    "| `apply_group::fold_events.scripted_turn` | 1,234,567 Ir | 1,100,000 Ir | +134,567 Ir (+12.2%) | ⚠️ more than 10% worse than main |",
  );
  expect(row(body, "`apply_group::one_event_on_long_thread.text_on_8k`")).toContain("| 0 |  |");
  const value = input();
  value.reducer = { pr: { state: "not-run" }, main: { state: "not-run" } };
  expect(renderComment(value)).toContain(
    "### Reducer instruction counts (`reducer-bench`)\n\nNot run.",
  );
});

test("a link is written only for an https URL without Markdown in it", () => {
  const value = input();
  value.scoreboard.windows.main = {
    state: "found",
    data:
      value.scoreboard.windows.pr.state === "found"
        ? value.scoreboard.windows.pr.data
        : ({} as never),
    url: "javascript:alert(1)",
    sha: "not a sha",
  };
  const body = renderComment(value);
  expect(body).toContain("main from its last successful run at an unknown commit");
  expect(body).not.toContain("javascript:");
});
