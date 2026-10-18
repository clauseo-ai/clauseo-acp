// The web half of `cargo xtask buildtime`: times the frontend's edit loop and prints one JSON
// object on stdout. xtask computes the statistics and compares them with budgets.toml; this script
// only measures. Notes for people go to stderr.
//
//   node ui/app/scripts/buildtime.mjs [--runs N]
//
// The commands are the scripts of ui/app/package.json: `typecheck` (tsc), `lint:types`
// (oxlint_type_aware), `build` (vite_build), and `dev` with `--port <p> --strictPort` (dev_start,
// hmr). Each runs as `process.execPath <its bin's JS entry> ...args`, so the timing has no shell
// and no pnpm in it, and it runs the same on Windows. Each timed probe gets one warm-up run, then N
// timed runs.
//
// The HMR probe edits src/App.tsx while it runs and always puts back its exact bytes: in `finally`,
// on SIGINT and SIGTERM, and at the start of the next run if this one was killed outright.
//
// Output: { "node": "v24.x", "runs": N, "probes": [ { "probe": name, ...one of } ] } where each
// probe carries exactly one of `samples_ms` (numbers), `bytes` (a number), `skipped` (why) or
// `failed` ({ "message", "command", "output" }).

import { spawn } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { initialJs, marked, parseRuns, scriptCommand } from "./buildtime-lib.mjs";

const APP_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const UI_DIR = resolve(APP_DIR, "..");
const APP_TSX = join(APP_DIR, "src", "App.tsx");
const MARK = /\s+data-buildtime="[^"]*"/g;

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
async function timedEdit(page, original, n) {
  const text = marked(original, n);
  const started = performance.now();
  writeFileSync(APP_TSX, text);
  await page.waitForFunction(
    (want) => document.querySelector("#app-root")?.getAttribute("data-buildtime") === want,
    String(n),
    { polling: "raf", timeout: EDIT_TIMEOUT_MS },
  );
  return performance.now() - started;
}

/** The dev_start and hmr probes. Leaves App.tsx as it found it and stops what it started. */
async function measureDev(runs, state) {
  const command = `node ui/app/scripts/buildtime.mjs --runs ${runs}`;
  const failed = (probe, error) => ({
    probe,
    failed: {
      message: `${probe} failed: ${String(error).split("\n")[0]}`,
      command,
      output: String(error),
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
      const original = state.original.toString("utf8");
      const samples = [];
      for (let run = 0; run <= runs; run += 1) {
        const ms = await timedEdit(page, original, run + 1);
        if (run > 0) {
          samples.push(ms);
        }
      }
      hmr = { probe: "hmr", samples_ms: samples };
    } catch (error) {
      hmr = failed("hmr", error);
    }
    return [devStart, hmr];
  } finally {
    await cleanUp(state);
  }
}

/** Puts App.tsx back to its original bytes, closes the browser and stops Vite. Safe to repeat. */
async function cleanUp(state) {
  restoreApp(state);
  const browser = state.browser;
  state.browser = undefined;
  if (browser !== undefined) {
    await browser.close().catch(() => {});
  }
  await stopAndWait(state.vite);
}

function restoreApp(state) {
  if (state.original !== undefined && !readFileSync(APP_TSX).equals(state.original)) {
    writeFileSync(APP_TSX, state.original);
  }
}

/** Removes a `data-buildtime` attribute that a killed run left in App.tsx. Returns the bytes. */
function recoverApp() {
  const text = readFileSync(APP_TSX, "utf8");
  if (!text.includes("data-buildtime=")) {
    return readFileSync(APP_TSX);
  }
  writeFileSync(APP_TSX, text.replace(MARK, ""));
  note("src/App.tsx held a data-buildtime attribute from an interrupted run; removed it");
  return readFileSync(APP_TSX);
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
  const state = { original: undefined, browser: undefined, vite: undefined, tool: undefined };
  const onSignal = (signal) => {
    restoreApp(state);
    stopChild(state.tool);
    stopChild(state.vite);
    process.exit(signal === "SIGINT" ? 130 : 143);
  };
  process.on("SIGINT", onSignal);
  process.on("SIGTERM", onSignal);
  state.original = recoverApp();

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
  process.stdout.write(`${JSON.stringify({ node: process.version, runs, probes })}\n`);
}

try {
  await main();
} catch (error) {
  process.stderr.write(
    `buildtime.mjs: ${error instanceof Error ? error.message : String(error)}\n`,
  );
  process.exitCode = 1;
}
