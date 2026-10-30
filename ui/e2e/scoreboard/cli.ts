// The scoreboard's command line, for the macOS `marks` job and for a local preview of the comment.
//
//   node ui/e2e/scoreboard/cli.ts summarise <os> <run-1.json> <run-2.json> <run-3.json>
//     Checks three measured runs (RawRun, metrics.ts) and prints scoreboard-<os>.json. The warm-up
//     run is played before and not passed.
//
//   node ui/e2e/scoreboard/cli.ts render <budgets.toml> [<name>=<file>]...
//     Prints the comment for local files instead of artifacts. Names: windows, windows-main, macos,
//     macos-main (scoreboard-<os>.json), reducer, reducer-main (reducer-bench-summary.json). A name
//     left out renders as "not run".

import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

import { parseBudgets, parseReducerSummaries, type ReducerCounts } from "./inputs.ts";
import {
  OSES,
  parseRawRun,
  parseResult,
  summarise,
  type Os,
  type ResultValues,
} from "./metrics.ts";
import { renderComment, type Source } from "./render.ts";

const USAGE =
  "usage: cli.ts summarise <windows|macos> <run-1.json> <run-2.json> <run-3.json>\n" +
  "       cli.ts render <budgets.toml> [windows|windows-main|macos|macos-main|reducer|reducer-main=<file>]...";

const LOCAL_SHA = "0".repeat(40);

function isOs(value: string | undefined): value is Os {
  return (OSES as readonly string[]).includes(value ?? "");
}

async function local<T>(
  files: Map<string, string>,
  name: string,
  parse: (text: string) => T,
): Promise<Source<T>> {
  const file = files.get(name);
  if (file === undefined) return { state: "not-run" };
  return { state: "found", data: parse(await readFile(file, "utf8")), url: "", sha: LOCAL_SHA };
}

export async function cli(args: readonly string[]): Promise<string> {
  const [command, first, ...rest] = args;
  if (command === "summarise" && isOs(first)) {
    const runs = await Promise.all(
      rest.map(async (file) => parseRawRun(JSON.parse(await readFile(file, "utf8")))),
    );
    return JSON.stringify(summarise(first, runs), null, 2) + "\n";
  }
  if (command === "render" && first !== undefined) {
    const files = new Map<string, string>();
    for (const arg of rest) {
      const [name, file] = arg.split("=", 2);
      if (name === undefined || file === undefined) throw new Error(USAGE);
      files.set(name, file);
    }
    const board = async (os: Os) => {
      const parse = (text: string): ResultValues => parseResult(text, os);
      return { pr: await local(files, os, parse), main: await local(files, `${os}-main`, parse) };
    };
    return renderComment({
      headSha: LOCAL_SHA,
      budgets: parseBudgets(await readFile(first, "utf8")),
      scoreboard: { windows: await board("windows"), macos: await board("macos") },
      reducer: {
        pr: await local<ReducerCounts>(files, "reducer", parseReducerSummaries),
        main: await local<ReducerCounts>(files, "reducer-main", parseReducerSummaries),
      },
    });
  }
  throw new Error(USAGE);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    process.stdout.write(await cli(process.argv.slice(2)));
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 2;
  }
}
