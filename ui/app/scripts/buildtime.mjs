// The web half of `cargo xtask buildtime`: times the frontend's edit loop and reports it as JSON.
// xtask computes the statistics and compares them with budgets.toml; this script only measures.
// The output contract (stdout, exit status, stderr) is in the module doc of
// xtask/src/buildtime/web.rs; change the two together.
//
//   node ui/app/scripts/buildtime.mjs [--runs N]
//
// The commands are the scripts of ui/app/package.json: `typecheck` (tsc), `lint:types`
// (oxlint_type_aware), `build` (vite_build), and `dev` with `--port <p> --strictPort` (dev_start,
// hmr). Each runs as `process.execPath <its bin's JS entry> ...args`, so the timing has no shell
// and no pnpm in it, and it runs the same on Windows. Each timed probe gets one warm-up run, then N
// timed runs.
//
// The HMR probe edits src/App.tsx while it runs, under the guard whose rules are written out in
// guard.mjs; buildtime-lib.mjs adds where the copy waits and which leftover edit is its own. Every child (the tools and Vite) starts, runs and stops
// through buildtime-proc.mjs, which stops it with everything it started.

import { existsSync, readFileSync } from "node:fs";
import { createServer } from "node:net";
import { dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import {
  appPaths,
  initialJs,
  marked,
  minNode,
  nodeAtLeast,
  parseRuns,
  recoverApp,
  scriptCommand,
} from "./buildtime-lib.mjs";
import { runTool, start, stop, stopAll } from "./buildtime-proc.mjs";
import { ConflictError, FileGuard, restoreFailure } from "./guard.mjs";

const APP_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const UI_DIR = resolve(APP_DIR, "..");
/** App.tsx, which the hmr probe edits, and its saved copy under the repo root. */
const APP_PATHS = appPaths(resolve(UI_DIR, ".."), "ui/app/src/App.tsx");

/** How long the dev server may take to print its "ready in" line, and an HMR edit to land. */
const READY_TIMEOUT_MS = 60_000;
const EDIT_TIMEOUT_MS = 10_000;
/** How long one tool run may take before it is stopped and its probe fails. */
const TOOL_TIMEOUT_MS = 120_000;
/**
 * How long the hmr probe waits, after an edit shows, before its next write. Vite 8.3.4's bundled
 * chokidar throttles each file's change events: the first one passes and starts a 50 ms window
 * (`_throttle(EV_CHANGE, path, 50)`, vite/dist/node/chunks/node.js:12264), and a change inside
 * that window is dropped, never delivered later. On windows-2025-runner an edit shows in about
 * 50 ms (48 to 80 ms in two CI runs of five edits), so without the wait the next write can fall
 * inside that window and its edit never shows. The wait is outside the timed span.
 */
const HMR_GAP_MS = 100;

function note(text) {
  process.stderr.write(`buildtime.mjs: ${text}\n`);
}

/**
 * The package.json script `script` timed: one warm-up run, then `runs` timed runs. A non-zero exit,
 * or a script that cannot run without a shell, makes the probe a failure.
 */
async function timeScript(probe, script, runs) {
  const command = `pnpm -C ui/app run ${script}`;
  let tool;
  try {
    tool = scriptCommand(APP_DIR, UI_DIR, script);
  } catch (error) {
    return { probe, failed: { message: String(error.message ?? error), command, output: "" } };
  }
  const samples = [];
  for (let run = 0; run <= runs; run += 1) {
    const result = await runTool(tool.argv, { cwd: APP_DIR, timeoutMs: TOOL_TIMEOUT_MS });
    if (result.timedOut) {
      const message = `\`${tool.text}\` did not finish within ${TOOL_TIMEOUT_MS / 1000} s; stopped it`;
      return { probe, failed: { message, command, output: result.output } };
    }
    if (result.code !== 0) {
      const message = `\`${tool.text}\` exited with ${result.code}`;
      return { probe, failed: { message, command, output: result.output } };
    }
    if (run > 0) {
      samples.push(result.ms);
    }
  }
  return { probe, samples_ms: samples };
}

/** A port nothing listens on now, from the OS. */
function freePort() {
  return new Promise((done, fail) => {
    const server = createServer();
    server.on("error", fail);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => done(port));
    });
  });
}

/**
 * Starts the dev server (the `dev` script) on `port`. Resolves once it prints "ready in", with the
 * time it took.
 */
function startVite(port, state) {
  return new Promise((done, fail) => {
    const { argv } = scriptCommand(APP_DIR, UI_DIR, "dev");
    const child = start([...argv, "--port", String(port), "--strictPort"], {
      cwd: APP_DIR,
      env: { ...process.env, NO_COLOR: "1" },
    });
    state.vite = child;
    let output = "";
    let ready = false;
    const timer = setTimeout(() => {
      void stop(child);
      fail(new Error(`vite printed no "ready in" line within ${READY_TIMEOUT_MS} ms\n${output}`));
    }, READY_TIMEOUT_MS);
    const onData = (chunk) => {
      output += chunk.toString("utf8");
      if (!ready && /ready in/.test(output)) {
        ready = true;
        clearTimeout(timer);
        done(performance.now() - child.startedAt);
      }
    };
    child.stdout.on("data", onData);
    child.stderr.on("data", onData);
    child.on("error", (error) => {
      clearTimeout(timer);
      fail(error);
    });
    child.on("exit", (code) => {
      clearTimeout(timer);
      if (!ready) {
        fail(new Error(`vite exited with ${code} before it was ready\n${output}`));
      }
    });
  });
}

/**
 * Why hmr fails when no browser launches: the Chrome channel finds no Chrome that starts, and no
 * CHROME_PATH is set. launchBrowser prints playwright's own error to stderr before it throws this.
 */
const NO_CHROME =
  "Google Chrome was not found at its standard paths, or would not launch; install it or set " +
  "CHROME_PATH to a Chrome or Chromium binary";

/**
 * Headless Google Chrome through playwright-core (`channel: "chrome"`, its standard install paths),
 * or the Chrome or Chromium binary at CHROME_PATH. Throws when neither launches: NO_CHROME for the
 * channel, playwright's own error for a CHROME_PATH binary. hmr fails either way, never skips.
 */
async function launchBrowser() {
  const { chromium } = await import("playwright-core");
  const chromePath = process.env.CHROME_PATH;
  if (chromePath !== undefined && chromePath !== "") {
    return chromium.launch({ headless: true, executablePath: chromePath });
  }
  try {
    return await chromium.launch({ headless: true, channel: "chrome" });
  } catch (error) {
    note(`could not launch Chrome: ${String(error).split("\n")[0]}`);
    throw new Error(NO_CHROME);
  }
}

/**
 * Writes the edit `n` and waits until the page's #app-root shows it. Resolves with the ms taken.
 */
async function timedEdit(page, guard, n) {
  const text = marked(guard.original.toString("utf8"), n);
  const started = performance.now();
  guard.write(text);
  await page.waitForFunction(
    (want) => document.querySelector("#app-root")?.getAttribute("data-buildtime") === want,
    String(n),
    { polling: "raf", timeout: EDIT_TIMEOUT_MS },
  );
  return performance.now() - started;
}

/** Records a conflict over App.tsx in `state.failures`, where it fails the run. */
function recordConflict(state, error) {
  if (error instanceof ConflictError) {
    state.failures.push({ message: error.message, fix: error.fix });
  }
}

/** The dev_start and hmr probes. Leaves App.tsx as it found it and stops what it started. */
async function measureDev(runs, state) {
  const command = `node ui/app/scripts/buildtime.mjs --runs ${runs}`;
  const failed = (probe, error) => ({
    probe,
    failed: {
      message: `${probe} failed: ${String(error.message ?? error).split("\n")[0]}`,
      command,
      output: String(error.stack ?? error),
    },
  });
  let devStart;
  let hmr;
  let port;
  try {
    try {
      // One warm-up start, then `runs` timed starts. Each start but the last is stopped; the last
      // server stays up for the HMR runs.
      const samples = [];
      for (let run = 0; run <= runs; run += 1) {
        port = await freePort();
        const ms = await startVite(port, state);
        if (run > 0) {
          samples.push(ms);
        }
        if (run < runs) {
          await stop(state.vite);
        }
      }
      devStart = { probe: "dev_start", samples_ms: samples };
    } catch (error) {
      return [
        failed("dev_start", error),
        { probe: "hmr", skipped: "the dev server did not start" },
      ];
    }
    try {
      state.browser = await launchBrowser();
      const page = await state.browser.newPage();
      await page.goto(`http://localhost:${port}/`);
      await page.waitForSelector("#app-root", { state: "attached" });
      // Checks ROOT_TAG before anything is saved or written.
      marked(readFileSync(APP_PATHS.file, "utf8"), 0);
      state.guard = new FileGuard(APP_PATHS);
      const samples = [];
      for (let run = 0; run <= runs; run += 1) {
        const ms = await timedEdit(page, state.guard, run + 1);
        if (run > 0) {
          samples.push(ms);
        }
        if (run < runs) {
          await delay(HMR_GAP_MS);
        }
      }
      hmr = { probe: "hmr", samples_ms: samples };
    } catch (error) {
      recordConflict(state, error);
      hmr = failed("hmr", error);
    }
    return [devStart, hmr];
  } finally {
    await cleanUp(state);
  }
}

/**
 * Puts App.tsx back under the guard's rule, closes the browser and stops Vite. Each step runs
 * whatever the one before it did: a failed restore becomes a run failure. Safe to repeat.
 */
async function cleanUp(state) {
  try {
    state.guard?.restore();
  } catch (error) {
    state.failures.push(restoreFailure(APP_PATHS, error));
  }
  const browser = state.browser;
  state.browser = undefined;
  if (browser !== undefined) {
    await browser.close().catch(() => {});
  }
  await stop(state.vite);
}

/** The output line: the contract is in xtask/src/buildtime/web.rs. */
function report(runs, probes, failures) {
  const out = { node: process.version, runs, probes };
  if (failures.length > 0) {
    out.failures = failures;
    process.exitCode = 1;
  }
  process.stdout.write(`${JSON.stringify(out)}\n`);
}

/**
 * On SIGINT, SIGTERM or SIGHUP: puts App.tsx back under the guard's rule (or names why it could
 * not), stops every child's process group (they are detached, so the terminal's signal does not
 * reach them), and exits with 128 + the signal's number. Playwright closes its browser as the
 * process exits. A second signal while it stops changes nothing.
 */
async function onSignal(state, signal) {
  if (state.stopping) {
    return;
  }
  state.stopping = true;
  try {
    state.guard?.restore();
  } catch (error) {
    const failure = restoreFailure(APP_PATHS, error);
    try {
      note(failure.message);
      note(`    fix: ${failure.fix}`);
    } catch {
      // stderr may be gone after SIGHUP; the copy stays for the next run either way.
    }
  }
  await stopAll();
  process.exit(128 + { SIGHUP: 1, SIGINT: 2, SIGTERM: 15 }[signal]);
}

async function main() {
  const runs = parseRuns(process.argv.slice(2));
  const min = minNode(UI_DIR);
  if (!nodeAtLeast(process.versions.node, min)) {
    const message = `Node ${process.versions.node} is too old for ui/ (engines: >=${min})`;
    report(runs, [], [{ message, fix: `install Node ${min} or later` }]);
    return;
  }
  if (!existsSync(join(APP_DIR, "node_modules"))) {
    throw new Error("ui/app/node_modules is missing; run `pnpm -C ui install --frozen-lockfile`");
  }
  const state = {
    guard: undefined,
    browser: undefined,
    vite: undefined,
    stopping: false,
    failures: [],
  };
  for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
    process.on(signal, () => onSignal(state, signal));
  }
  try {
    const line = recoverApp(APP_PATHS);
    if (line !== undefined) {
      note(line);
    }
  } catch (error) {
    if (!(error instanceof ConflictError)) {
      throw error;
    }
    report(runs, [], [{ message: error.message, fix: error.fix }]);
    return;
  }

  const probes = [
    await timeScript("tsc", "typecheck", runs),
    await timeScript("oxlint_type_aware", "lint:types", runs),
  ];
  const build = await timeScript("vite_build", "build", runs);
  probes.push(build);
  if (build.failed === undefined) {
    try {
      probes.push({ probe: "initial_js", ...initialJs(join(APP_DIR, "dist")) });
    } catch (error) {
      const message = `could not size the initial JS: ${String(error)}`;
      probes.push({
        probe: "initial_js",
        failed: { message, command: "pnpm -C ui/app run build", output: "" },
      });
    }
  } else {
    probes.push({ probe: "initial_js", skipped: "vite build failed" });
  }
  probes.push(...(await measureDev(runs, state)));
  report(runs, probes, state.failures);
}

try {
  await main();
} catch (error) {
  process.stderr.write(
    `buildtime.mjs: ${error instanceof Error ? error.message : String(error)}\n`,
  );
  process.exitCode = 1;
} finally {
  await stopAll();
}
