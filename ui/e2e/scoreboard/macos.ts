// The speed scoreboard on macOS (R13). The `marks` build of the app plays the scoreboard session
// by itself (the page's autoplay, ui/app/src/perf/) and writes one RawRun (metrics.ts) to the
// file in MARKS_OUT before it quits. This runner launches it once to warm up and three times
// measured, each launch on new scratch folders (a CLAUSEO_DATA_DIR whose settings.json starts
// fake_agent on the `scoreboard` scenario at --pace-ms 5, and an E2E_MATTER_DIR, as the Windows
// fixture makes them: app-windows/scratch.ts), checks each run, and writes the medians to
// scoreboard-macos.json. Plain Node 24 runs it, no install: it imports Node's own modules and
// files of ui/e2e only.
//
//   MARKS_APP        path to the marks build's binary (required)
//   MARKS_FAKE_AGENT absolute path to the release fake_agent (required)
//   SCOREBOARD_OUT   folder that receives scoreboard-macos.json (required), and in runs/ each
//                    play's run file as the app wrote it (play-1-warmup.json, play-2.json, ...)
//
//   MARKS_APP=target/release/clauseo-desktop MARKS_FAKE_AGENT=$PWD/target/release/fake_agent \
//     SCOREBOARD_OUT=out node ui/e2e/scoreboard/macos.ts

import { spawn } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { isAbsolute, join } from "node:path";
import { pathToFileURL } from "node:url";

import { makeScratch, removeScratch, scratchEnv, type Scratch } from "../app-windows/scratch.ts";
import { MEASURED_RUNS, WARMUP_RUNS, parseRawRun, summarise, type RawRun } from "./metrics.ts";
import { PACE_ARGS, RUNS_DIR, SCENARIO, playName, resultTable } from "./session.ts";

/**
 * How long one play may take, launch to exit. The session streams for about 15 s at 5 ms a step;
 * the rest is the launch, the idle settle and a slow runner.
 */
export const PLAY_TIMEOUT_MS = 360_000;
/** The file the app writes its run to, in each play's scratch root. */
const RUN_FILE = "run.json";

/** What the runner needs from its environment. */
export interface Config {
  app: string;
  fakeAgent: string;
  out: string;
}

/** Reads {@link Config} from `env`; throws naming every variable that is missing or wrong. */
export function config(env: NodeJS.ProcessEnv): Config {
  const problems: string[] = [];
  const get = (name: string): string => {
    const value = env[name];
    if (value === undefined || value === "") problems.push(`${name} is not set`);
    return value ?? "";
  };
  const app = get("MARKS_APP");
  const fakeAgent = get("MARKS_FAKE_AGENT");
  const out = get("SCOREBOARD_OUT");
  if (fakeAgent !== "" && !isAbsolute(fakeAgent)) {
    problems.push(`MARKS_FAKE_AGENT must be an absolute path, not ${fakeAgent}`);
  }
  if (problems.length > 0) throw new Error(problems.join("; "));
  return { app, fakeAgent, out };
}

/** One play's scratch folders: the fake_agent settings and an empty matter folder. */
export function playScratch(cfg: Config, base?: string): Scratch {
  return makeScratch(
    { scenario: SCENARIO, matterFiles: {}, agentArgs: PACE_ARGS },
    () => cfg.fakeAgent,
    base,
  );
}

/** The variables a play's launch adds: the scratch folders, and MARKS_OUT in the scratch root. */
export function playEnv(scratch: Scratch): Record<string, string> {
  return { ...scratchEnv(scratch), MARKS_OUT: join(scratch.root, RUN_FILE) };
}

/**
 * Runs `command` with `env` added until it exits; kills it (its own PID, SIGKILL) after
 * `timeoutMs`. Resolves to the exit code; rejects on a failed start, a signal or the timeout.
 */
export async function runToExit(
  command: string,
  args: readonly string[],
  env: Record<string, string>,
  timeoutMs: number,
): Promise<number> {
  const child = spawn(command, args, { env: { ...process.env, ...env }, stdio: "inherit" });
  return await new Promise<number>((resolve, reject) => {
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill("SIGKILL");
    }, timeoutMs);
    child.once("error", (error) => {
      clearTimeout(timer);
      reject(new Error(`${command} did not start: ${error.message}`));
    });
    child.once("exit", (code, signal) => {
      clearTimeout(timer);
      if (timedOut) reject(new Error(`${command} did not exit within ${timeoutMs} ms; killed it`));
      else if (code === null) reject(new Error(`${command} ended by ${String(signal)}`));
      else resolve(code);
    });
  });
}

/** One play: a fresh launch on new scratch folders, its run read and checked. */
export async function play(
  cfg: Config,
  n: number,
  { timeoutMs = PLAY_TIMEOUT_MS, base }: { timeoutMs?: number; base?: string } = {},
): Promise<RawRun> {
  const scratch = playScratch(cfg, base);
  try {
    const env = playEnv(scratch);
    const started = performance.now();
    const code = await runToExit(cfg.app, [], env, timeoutMs);
    const file = env["MARKS_OUT"]!;
    // Kept as written, before any check, so a run the checks refuse can still be read.
    if (existsSync(file)) {
      const runs = join(cfg.out, RUNS_DIR);
      mkdirSync(runs, { recursive: true });
      copyFileSync(file, join(runs, `${playName(n)}.json`));
    }
    if (code !== 0) throw new Error(`play ${n}: the app exited with ${code} (its stderr says why)`);
    const run = parseRawRun(JSON.parse(readFileSync(file, "utf8")));
    process.stdout.write(
      `[scoreboard] play ${n} done in ${Math.round(performance.now() - started)} ms\n`,
    );
    return run;
  } finally {
    if (process.env["E2E_KEEP_SCRATCH"] === "1") {
      process.stdout.write(`[scoreboard] scratch folders kept in ${scratch.root}\n`);
    } else {
      removeScratch(scratch);
    }
  }
}

/** The whole runner: the plays, the result file, the table. */
export async function main(env: NodeJS.ProcessEnv): Promise<void> {
  const cfg = config(env);
  const measured: RawRun[] = [];
  for (let n = 1; n <= WARMUP_RUNS + MEASURED_RUNS; n++) {
    const run = await play(cfg, n);
    if (n > WARMUP_RUNS) measured.push(run);
  }
  const result = summarise("macos", measured);
  mkdirSync(cfg.out, { recursive: true });
  const file = join(cfg.out, "scoreboard-macos.json");
  writeFileSync(file, `${JSON.stringify(result, null, 2)}\n`);
  process.stdout.write(resultTable(result));
  process.stdout.write(`[scoreboard] wrote ${file}\n`);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    await main(process.env);
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  }
}
