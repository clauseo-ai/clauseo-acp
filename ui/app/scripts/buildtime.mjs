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
// The HMR probe edits src/App.tsx while it runs, under the guard in buildtime-lib.mjs: a copy of
// the original under target/xtask/buildtime/ first, every write through a temporary file, and the
// original put back only over bytes this script wrote, in `finally` and on SIGINT, SIGTERM and
// SIGHUP. A killed run is settled at the start of the next one (`recoverApp`). A developer's edit
// is never overwritten: both files stay, and the conflict fails the run.

import { spawn } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { createServer } from "node:net";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  AppGuard,
  ConflictError,
  appPaths,
  initialJs,
  marked,
  parseRuns,
  recoverApp,
  scriptCommand,
} from "./buildtime-lib.mjs";

const APP_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const UI_DIR = resolve(APP_DIR, "..");
/** App.tsx, which the hmr probe edits, and its saved copy under the repo root. */
const APP_PATHS = appPaths(resolve(UI_DIR, ".."), "ui/app/src/App.tsx");

/** How long the dev server may take to print its "ready in" line, and an HMR edit to land. */
const READY_TIMEOUT_MS = 60_000;
const EDIT_TIMEOUT_MS = 10_000;
/** How long one tool run may take before it is stopped and its probe fails. */
const TOOL_TIMEOUT_MS = 120_000;
/** How long a child has between SIGTERM and SIGKILL, and how long a stop waits for it to exit. */
const KILL_GRACE_MS = 5_000;
const EXIT_WAIT_MS = 5_000;

function note(text) {
  process.stderr.write(`buildtime.mjs: ${text}\n`);
}

/**
 * Stops a child this script started, by its handle: SIGTERM, then SIGKILL after KILL_GRACE_MS if it
 * is still running. On Windows, `taskkill /T /F` on the child's PID, which takes its tree.
 */
function stopChild(child) {
  if (child === undefined || child.exitCode !== null || child.signalCode !== null) {
    return;
  }
  if (process.platform === "win32") {
    spawn("taskkill", ["/pid", String(child.pid), "/T", "/F"], { stdio: "ignore" });
    return;
  }
  child.kill("SIGTERM");
  const timer = setTimeout(() => {
    if (child.exitCode === null && child.signalCode === null) {
      child.kill("SIGKILL");
    }
  }, KILL_GRACE_MS);
  timer.unref();
  child.once("exit", () => clearTimeout(timer));
}

/** Stops `child` and waits up to EXIT_WAIT_MS for it to exit. Resolves with whether it did. */
function stopAndWait(child) {
  if (child === undefined || child.exitCode !== null || child.signalCode !== null) {
    return Promise.resolve(true);
  }
  return new Promise((done) => {
    const timer = setTimeout(() => {
      if (child.exitCode === null && child.signalCode === null && process.platform !== "win32") {
        child.kill("SIGKILL");
      }
      note(`pid ${child.pid} did not exit within ${EXIT_WAIT_MS} ms of SIGTERM; sent SIGKILL`);
      done(false);
    }, EXIT_WAIT_MS);
    child.once("exit", () => {
      clearTimeout(timer);
      done(true);
    });
    stopChild(child);
  });
}

/**
 * Runs `node ...argv` in ui/app. Resolves with its exit code, wall time and output, and
 * `timedOut` when it ran past TOOL_TIMEOUT_MS and was stopped.
 */
function runTool(argv, state) {
  return new Promise((done) => {
    const started = performance.now();
    const child = spawn(process.execPath, argv, {
      cwd: APP_DIR,
      stdio: ["ignore", "pipe", "pipe"],
    });
    state.tool = child;
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      stopChild(child);
    }, TOOL_TIMEOUT_MS);
    const chunks = [];
    child.stdout.on("data", (chunk) => chunks.push(chunk));
    child.stderr.on("data", (chunk) => chunks.push(chunk));
    child.on("error", (error) => {
      clearTimeout(timer);
      done({ code: -1, ms: performance.now() - started, output: String(error), timedOut });
    });
    child.on("close", (code) => {
      clearTimeout(timer);
      state.tool = undefined;
      const ms = performance.now() - started;
      done({ code, ms, output: Buffer.concat(chunks).toString("utf8"), timedOut });
    });
  });
}

/**
 * The package.json script `script` timed: one warm-up run, then `runs` timed runs. A non-zero exit,
 * or a script that cannot run without a shell, makes the probe a failure.
 */
async function timeScript(probe, script, runs, state) {
  const command = `pnpm -C ui/app run ${script}`;
  let tool;
  try {
    tool = scriptCommand(APP_DIR, UI_DIR, script);
  } catch (error) {
    return { probe, failed: { message: String(error.message ?? error), command, output: "" } };
  }
  const samples = [];
  for (let run = 0; run <= runs; run += 1) {
    const result = await runTool(tool.argv, state);
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
    const started = performance.now();
    const child = spawn(process.execPath, [...argv, "--port", String(port), "--strictPort"], {
      cwd: APP_DIR,
      stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env, NO_COLOR: "1" },
    });
    state.vite = child;
    let output = "";
    let ready = false;
    const timer = setTimeout(() => {
      stopChild(child);
      fail(new Error(`vite printed no "ready in" line within ${READY_TIMEOUT_MS} ms\n${output}`));
    }, READY_TIMEOUT_MS);
    const onData = (chunk) => {
      output += chunk.toString("utf8");
      if (!ready && /ready in/.test(output)) {
        ready = true;
        clearTimeout(timer);
        done(performance.now() - started);
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

/** Why hmr is skipped when no browser launches. Only CHROME_PATH finds a Chromium. */
const NO_CHROME =
  "Google Chrome not found at its standard paths; set CHROME_PATH to a Chrome or Chromium binary";

/**
 * Headless Google Chrome through playwright-core (`channel: "chrome"`, its standard install paths),
 * or the Chrome or Chromium binary at CHROME_PATH. `undefined` when none can be launched.
 */
async function launchBrowser() {
  const { chromium } = await import("playwright-core");
  const chromePath = process.env.CHROME_PATH;
  const options =
    chromePath === undefined || chromePath === ""
      ? { headless: true, channel: "chrome" }
      : { headless: true, executablePath: chromePath };
  try {
    return await chromium.launch(options);
  } catch (error) {
    note(`could not launch Chrome: ${String(error).split("\n")[0]}`);
    return undefined;
  }
}

/** Writes the edit `n` and waits until the page's #app-root shows it. Resolves with the ms taken. */
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
          await stopAndWait(state.vite);
        }
      }
      devStart = { probe: "dev_start", samples_ms: samples };
    } catch (error) {
      return [
        failed("dev_start", error),
        { probe: "hmr", skipped: "the dev server did not start" },
      ];
    }
    state.browser = await launchBrowser();
    if (state.browser === undefined) {
      return [devStart, { probe: "hmr", skipped: NO_CHROME }];
    }
    try {
      const page = await state.browser.newPage();
      await page.goto(`http://localhost:${port}/`);
      await page.waitForSelector("#app-root", { state: "attached" });
      // Checks ROOT_TAG before anything is saved or written.
      marked(readFileSync(APP_PATHS.app, "utf8"), 0);
      state.guard = new AppGuard(APP_PATHS);
      const samples = [];
      for (let run = 0; run <= runs; run += 1) {
        const ms = await timedEdit(page, state.guard, run + 1);
        if (run > 0) {
          samples.push(ms);
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

/** Puts App.tsx back under the guard's rule, closes the browser and stops Vite. Safe to repeat. */
async function cleanUp(state) {
  try {
    state.guard?.restore();
  } catch (error) {
    recordConflict(state, error);
    if (!(error instanceof ConflictError)) {
      throw error;
    }
  }
  const browser = state.browser;
  state.browser = undefined;
  if (browser !== undefined) {
    await browser.close().catch(() => {});
  }
  await stopAndWait(state.vite);
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
 * On SIGINT, SIGTERM or SIGHUP: puts App.tsx back under the guard's rule (or names the conflict),
 * stops the children, and exits with 128 + the signal's number. Playwright closes its browser as
 * the process exits.
 */
function onSignal(state, signal) {
  try {
    state.guard?.restore();
  } catch (error) {
    try {
      note(error.message);
      if (error.fix !== undefined) {
        note(`    fix: ${error.fix}`);
      }
    } catch {
      // stderr may be gone after SIGHUP; the copy stays for the next run either way.
    }
  }
  stopChild(state.tool);
  stopChild(state.vite);
  process.exit(128 + { SIGHUP: 1, SIGINT: 2, SIGTERM: 15 }[signal]);
}

async function main() {
  const major = Number(process.versions.node.split(".")[0]);
  if (major < 24) {
    throw new Error(`Node ${process.versions.node} is too old; install Node 24 or later`);
  }
  const runs = parseRuns(process.argv.slice(2));
  if (!existsSync(join(APP_DIR, "node_modules"))) {
    throw new Error("ui/app/node_modules is missing; run `pnpm -C ui install --frozen-lockfile`");
  }
  const state = {
    guard: undefined,
    browser: undefined,
    vite: undefined,
    tool: undefined,
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
    await timeScript("tsc", "typecheck", runs, state),
    await timeScript("oxlint_type_aware", "lint:types", runs, state),
  ];
  const build = await timeScript("vite_build", "build", runs, state);
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
}
