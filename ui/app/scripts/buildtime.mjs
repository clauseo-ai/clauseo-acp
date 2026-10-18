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

function note(text) {
  process.stderr.write(`buildtime.mjs: ${text}\n`);
}

/** Runs `node ...argv` in ui/app. Resolves with its exit code, wall time and output. */
function runTool(argv) {
  return new Promise((done) => {
    const started = performance.now();
    const child = spawn(process.execPath, argv, {
      cwd: APP_DIR,
      stdio: ["ignore", "pipe", "pipe"],
    });
    const chunks = [];
    child.stdout.on("data", (chunk) => chunks.push(chunk));
    child.stderr.on("data", (chunk) => chunks.push(chunk));
    child.on("error", (error) => {
      done({ code: -1, ms: performance.now() - started, output: String(error) });
    });
    child.on("close", (code) => {
      const ms = performance.now() - started;
      done({ code, ms, output: Buffer.concat(chunks).toString("utf8") });
    });
  });
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
    const result = await runTool(tool.argv);
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

/** Stops the Vite process this script started, by its handle; on Windows its whole tree. */
function stopChild(child) {
  if (child === undefined || child.exitCode !== null || child.signalCode !== null) {
    return;
  }
  if (process.platform === "win32") {
    spawn("taskkill", ["/pid", String(child.pid), "/T", "/F"], { stdio: "ignore" });
  } else {
    child.kill("SIGTERM");
  }
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

/** Headless Chrome through playwright-core, or `undefined` when none can be launched. */
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
  try {
    const port = await freePort();
    try {
      devStart = { probe: "dev_start", samples_ms: [await startVite(port, state)] };
    } catch (error) {
      return [
        failed("dev_start", error),
        { probe: "hmr", skipped: "the dev server did not start" },
      ];
    }
    state.browser = await launchBrowser();
    if (state.browser === undefined) {
      return [devStart, { probe: "hmr", skipped: "no Chrome or Chromium found (set CHROME_PATH)" }];
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
  const vite = state.vite;
  if (vite !== undefined && vite.exitCode === null && vite.signalCode === null) {
    const exited = new Promise((done) => vite.once("exit", done));
    stopChild(vite);
    await exited;
  }
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
  const state = { original: undefined, browser: undefined, vite: undefined };
  const onSignal = (signal) => {
    restoreApp(state);
    stopChild(state.vite);
    process.exit(signal === "SIGINT" ? 130 : 143);
  };
  process.on("SIGINT", onSignal);
  process.on("SIGTERM", onSignal);
  state.original = recoverApp();

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
