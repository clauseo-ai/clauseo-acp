// The scoreboard's pull request comment: one Markdown body, marked so the workflow finds it again.
// Every number in it is checked before it gets here (metrics.ts, inputs.ts); the only text from an
// artifact that reaches the comment is a reducer case name, which must match a strict pattern.

import type { Budget, ReducerCounts } from "./inputs.ts";
import { METRICS, OSES, type Os, type ResultValues, type Unit } from "./metrics.ts";

/** The first line of the comment. The workflow edits the comment whose body starts with it. */
export const MARKER = "<!-- scoreboard -->";
/** A row is flagged when it is more than this fraction worse than main. */
export const WORSE_THAN_MAIN = 0.1;

/** Where one set of numbers came from, or why there are none. */
export type Source<T> =
  /** An artifact of a run, read and checked. `sha` is the commit the run measured. */
  | { state: "found"; data: T; url: string; sha: string }
  /** No artifact yet, and a run that may still upload it is in progress. */
  | { state: "pending"; url: string }
  /** No run uploaded it: the job did not run, failed before uploading, or does not exist yet. */
  | { state: "not-run" }
  /** An artifact that failed the checks. `reason` is this code's own text, never the artifact's. */
  | { state: "invalid"; url: string; reason: string };

export interface RenderInput {
  /** The pull request's head commit. */
  headSha: string;
  budgets: Map<string, Budget>;
  scoreboard: Record<Os, { pr: Source<ResultValues>; main: Source<ResultValues> }>;
  reducer: { pr: Source<ReducerCounts>; main: Source<ReducerCounts> };
}

const OS_LABEL: Record<Os, string> = {
  windows: "Windows x64 (`windows-2025`, WebView2)",
  macos: "macOS Apple silicon (`macos-15`, WebKit)",
};

/** Up to one decimal, without a trailing `.0`; thousands separated. */
export function formatNumber(value: number): string {
  const rounded = Math.round(value * 10) / 10;
  return rounded.toLocaleString("en-US", { maximumFractionDigits: 1 });
}

/** The number and its unit; a count goes without one. */
function withUnit(value: number, unit: string): string {
  return unit === "count" ? formatNumber(value) : `${formatNumber(value)} ${unit}`;
}

/** The flags of one row: over its budget, more than 10% worse than main, both, or none. */
export function flags(pr: number | null, main: number | null, limit: number | null): string[] {
  const out: string[] = [];
  if (pr === null) return out;
  if (limit !== null && pr > limit) out.push("over budget");
  if (main !== null && pr > main * (1 + WORSE_THAN_MAIN)) out.push("more than 10% worse than main");
  return out;
}

/** The change from main: the difference, and the percentage when main is above zero. */
export function change(pr: number | null, main: number | null, unit: string): string {
  if (pr === null || main === null) return "–";
  const diff = pr - main;
  if (Math.round(diff * 10) === 0) return "0";
  const sign = diff > 0 ? "+" : "−";
  const abs = withUnit(Math.abs(diff), unit);
  if (main <= 0) return `${sign}${abs}`;
  const pct = Math.abs((diff / main) * 100).toFixed(1);
  return `${sign}${abs} (${sign}${pct}%)`;
}

const SHA = /^[0-9a-f]{40}$/;
const URL_OK = /^https:\/\/[A-Za-z0-9._~:/?#@!$&'*+,;=%-]+$/;

function link(text: string, url: string): string {
  return URL_OK.test(url) ? `[${text}](${url})` : text;
}

function short(sha: string): string {
  return SHA.test(sha) ? `\`${sha.slice(0, 7)}\`` : "an unknown commit";
}

/** The cell for a source with no number: why there is none. */
function missing<T>(source: Source<T>): string {
  switch (source.state) {
    case "pending":
      return "pending";
    case "invalid":
      return "invalid result";
    default:
      return "not run";
  }
}

/** One line naming the runs a section's numbers came from. */
function provenance<T>(pr: Source<T>, main: Source<T>, what: string): string {
  const prPart =
    pr.state === "found"
      ? `this PR from ${link(`its ${what} run`, pr.url)}`
      : pr.state === "pending"
        ? `this PR's ${link(`${what} run`, pr.url)} is still running`
        : pr.state === "invalid"
          ? `this PR's ${link(`${what} result`, pr.url)} was refused: ${pr.reason}`
          : `this PR has no ${what} result`;
  const mainPart =
    main.state === "found"
      ? `main from ${link("its last successful run", main.url)} at ${short(main.sha)}`
      : main.state === "invalid"
        ? `main's ${link("last result", main.url)} was refused: ${main.reason}`
        : "main has no successful run with this result";
  return `Numbers: ${prPart}; ${mainPart}.`;
}

function scoreboardSection(input: RenderInput, os: Os): string[] {
  const { pr, main } = input.scoreboard[os];
  const lines = [
    `### ${OS_LABEL[os]}`,
    "",
    provenance(pr, main, "scoreboard"),
    "",
    "| Metric | This PR | main | Change | Budget | |",
    "|---|---|---|---|---|---|",
  ];
  for (const metric of METRICS) {
    const budget = input.budgets.get(metric.id);
    const limit = budget !== undefined && budget.unit === metric.unit ? budget.limit : null;
    const prValue = pr.state === "found" ? pr.data[metric.id] : null;
    const mainValue = main.state === "found" ? main.data[metric.id] : null;
    const cell = (source: Source<ResultValues>, value: number | null, unit: Unit) =>
      source.state !== "found"
        ? missing(source)
        : value === null
          ? "not measured"
          : withUnit(value, unit);
    const flagged = flags(prValue, mainValue, limit);
    lines.push(
      `| ${metric.label} | ${cell(pr, prValue, metric.unit)} | ${cell(main, mainValue, metric.unit)} | ` +
        `${change(prValue, mainValue, metric.unit)} | ${limit === null ? "–" : withUnit(limit, metric.unit)} | ` +
        `${flagged.length > 0 ? `⚠️ ${flagged.join(", ")}` : ""} |`,
    );
  }
  return lines;
}

function reducerSection(input: RenderInput): string[] {
  const { pr, main } = input.reducer;
  const lines = ["### Reducer instruction counts (`reducer-bench`)", ""];
  const cases = new Set<string>();
  if (pr.state === "found") for (const name of pr.data.keys()) cases.add(name);
  if (main.state === "found") for (const name of main.data.keys()) cases.add(name);
  if (pr.state !== "found" && cases.size === 0) {
    lines.push(
      `${missing(pr) === "pending" ? "Pending" : "Not run"}. ${provenance(pr, main, "reducer-bench")}`,
    );
    return lines;
  }
  lines.push(
    provenance(pr, main, "reducer-bench") +
      " Counted under Valgrind, so they repeat exactly; reducer-bench itself fails a pull request on " +
      "more than 2% more in any case.",
    "",
    "| Case | This PR | main | Change | |",
    "|---|---|---|---|---|",
  );
  for (const name of [...cases].sort()) {
    const prValue = pr.state === "found" ? (pr.data.get(name) ?? null) : null;
    const mainValue = main.state === "found" ? (main.data.get(name) ?? null) : null;
    const cell = (source: Source<ReducerCounts>, value: number | null) =>
      source.state !== "found"
        ? missing(source)
        : value === null
          ? "no such case"
          : withUnit(value, "Ir");
    const flagged = flags(prValue, mainValue, null);
    lines.push(
      `| \`${name}\` | ${cell(pr, prValue)} | ${cell(main, mainValue)} | ${change(prValue, mainValue, "Ir")} | ` +
        `${flagged.length > 0 ? `⚠️ ${flagged.join(", ")}` : ""} |`,
    );
  }
  return lines;
}

/** The whole comment body. */
export function renderComment(input: RenderInput): string {
  const lines = [
    MARKER,
    "## Speed scoreboard",
    "",
    `For ${short(input.headSha)}, the head of this pull request. The fixed \`scoreboard\` session ` +
      "(`fake_agent --pace-ms 5`) played into the app; each number is the median of three runs after one " +
      "warm-up. Wall-clock numbers report and alert; they never block a merge. ⚠️ marks a row over its " +
      "budget (budgets.toml) or more than 10% worse than main.",
    "",
  ];
  for (const os of OSES) lines.push(...scoreboardSection(input, os), "");
  lines.push(...reducerSection(input), "");
  lines.push(
    "<sub>Written by `.github/workflows/scoreboard-comment.yml` when a CI or e2e-windows run of this " +
      "pull request ends; each run edits this comment.</sub>",
  );
  return lines.join("\n") + "\n";
}
