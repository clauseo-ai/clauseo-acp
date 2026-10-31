// The real app on Windows under Playwright. The e2e build (`--features e2e`) opens WebView2's
// DevTools port on a port the browser picks, which it writes to `EBWebView/DevToolsActivePort` in
// its data folder; the fixture starts the app with a new WebView2 data folder, reads the port
// from that file, attaches with `connectOverCDP`, hands the test the app's page, then closes the
// connection, ends the app's whole process tree by its PID and deletes the folders.
//
// Every launch runs on the test's scratch folders (scratch.ts): `CLAUSEO_DATA_DIR` is a new data
// folder, never the real one, and `E2E_MATTER_DIR` the matter folder. A spec picks fake_agent's
// scenario and the matter's files with `test.use({ scenario, matterFiles })`; `apps` starts and
// stops more launches on the same folders (the restart spec).
//
//   E2E_APP           path to the e2e build of clauseo-desktop.exe (required)
//   E2E_FAKE_AGENT    path to fake_agent.exe (default: beside E2E_APP; needed with a scenario)
//   E2E_DISABLE_GPU   `1` runs the webview with --disable-gpu; the app reads it too
//   E2E_KEEP_SCRATCH  `1` keeps the scratch folders and prints where they are

import { execFile, spawn, type ChildProcess } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { setTimeout as wait } from "node:timers/promises";
import { promisify } from "node:util";
import { chromium, test as base, type Browser, type Page } from "@playwright/test";
import {
  fakeAgentPath,
  makeScratch,
  removeScratch,
  scratchEnv,
  webviewFolder,
  type Scratch,
} from "./scratch.ts";

/** The time from starting the app to having its page: the port file, connecting, the page. */
const SETUP_TIMEOUT_MS = 60_000;
/** How long `taskkill` may take. */
const KILL_TIMEOUT_MS = 30_000;
/** How long the app may take to exit after its window is asked to close. */
const CLOSE_TIMEOUT_MS = 30_000;
/** The pause between two tries while WebView2 writes its port file, listens or opens its page. */
const RETRY_MS = 200;

/** How the webview runs, from `E2E_DISABLE_GPU`. */
export type Mode = "normal" | "disable-gpu";

/** The running app, as the test sees it. */
export interface App {
  /** The page of the app's one window. */
  page: Page;
  /** The CDP connection to the app's WebView2. */
  browser: Browser;
  /** The DevTools port, from `DevToolsActivePort`. */
  port: number;
  mode: Mode;
  /** The app's PID: the root of its process tree, WebView2's processes included. */
  pid: number;
  /** `performance.now()` just before the app was started. */
  startedAt: number;
  /** The temporary WebView2 data folder (`WEBVIEW2_USER_DATA_FOLDER`), new for each launch. */
  userDataFolder: string;
  /** Which launch on this test's folders: 1 for the first. */
  launch: number;
}

/** How a launch ends: its window asked to close (as a user would), or its tree killed. */
export type Stop = "close" | "kill";

/** Launches of the app on one test's scratch folders. */
export interface Apps {
  scratch: Scratch;
  /** Starts the app once more on the same data and matter folders, with a new WebView2 folder. */
  start(): Promise<App>;
  /**
   * Ends a launch and waits for it to exit. `close` sends the window WM_CLOSE (`taskkill` without
   * `/F`) so the app shuts down as it does for a user, and kills the tree only if it has not
   * exited within CLOSE_TIMEOUT_MS, with a warning on the run.
   */
  stop(app: App, how?: Stop): Promise<void>;
}

function appPath(): string {
  const path = process.env["E2E_APP"];
  if (path === undefined || path === "") {
    throw new Error("E2E_APP is not set; set it to the e2e build of clauseo-desktop.exe");
  }
  return path;
}

function mode(): Mode {
  return process.env["E2E_DISABLE_GPU"] === "1" ? "disable-gpu" : "normal";
}

/** The milliseconds left before `deadline`; throws, naming `step`, once it has passed. */
function remaining(deadline: number, step: string): number {
  const left = deadline - performance.now();
  if (left <= 0) {
    throw new Error(`${step} did not finish within the ${SETUP_TIMEOUT_MS} ms setup time`);
  }
  return left;
}

/** The names in `folder`, for an error message. */
function entries(folder: string): string {
  try {
    return readdirSync(folder).join(", ") || "nothing";
  } catch (error) {
    return `nothing readable (${String(error)})`;
  }
}

/**
 * The port from the first line of `<userDataFolder>/EBWebView/DevToolsActivePort`, which WebView2
 * writes once it listens on the port that `--remote-debugging-port=0` let it pick. The folder is
 * new for each launch, so the file is never left from an earlier one. Rejects if the app does not
 * start, exits, or writes no file within the setup time.
 */
async function readPort(
  child: ChildProcess,
  userDataFolder: string,
  deadline: number,
): Promise<number> {
  const file = join(userDataFolder, "EBWebView", "DevToolsActivePort");
  let failure: Error | undefined;
  const onError = (error: Error): void => {
    failure = new Error(`the app did not start: ${appPath()}`, { cause: error });
  };
  const onExit = (code: number | null): void => {
    failure ??= new Error(`the app exited with ${String(code)} before WebView2 wrote ${file}`);
  };
  child.once("error", onError);
  child.once("exit", onExit);
  try {
    for (;;) {
      if (failure !== undefined) {
        throw failure;
      }
      const port = portIn(file);
      if (port !== undefined) {
        return port;
      }
      if (performance.now() + RETRY_MS > deadline) {
        throw new Error(
          `WebView2 wrote no port to ${file} within the ${SETUP_TIMEOUT_MS} ms setup time;` +
            ` EBWebView holds: ${entries(join(userDataFolder, "EBWebView"))}`,
        );
      }
      await wait(RETRY_MS);
    }
  } finally {
    // The "error" handler stays: a ChildProcess "error" with no handler would end the worker.
    child.off("exit", onExit);
  }
}

/**
 * The read errors that mean "not yet": ENOENT before WebView2 creates the file, EBUSY while it
 * holds the file open to write it (seen on windows-2025, run 41168327806).
 */
const NOT_YET = new Set(["ENOENT", "EBUSY"]);

/** The port on the first line of `file`, or undefined while the file is missing or being written. */
function portIn(file: string): number | undefined {
  let text: string;
  try {
    text = readFileSync(file, "utf8");
  } catch (error) {
    if (NOT_YET.has((error as NodeJS.ErrnoException).code ?? "")) {
      return undefined;
    }
    throw error;
  }
  // The line ends with a newline once the port is whole; a file caught mid-write waits a round.
  const match = /^(\d+)\r?\n/.exec(text);
  return match?.[1] === undefined ? undefined : Number(match[1]);
}

/** Connects to WebView2 on `port`, retrying while it opens the port. */
async function connect(port: number, deadline: number): Promise<Browser> {
  for (;;) {
    try {
      return await chromium.connectOverCDP(`http://127.0.0.1:${port}`, {
        timeout: remaining(deadline, "connecting over CDP"),
      });
    } catch (error) {
      if (performance.now() + RETRY_MS > deadline) {
        throw new Error(
          `connecting over CDP to port ${port} did not finish within the setup time`,
          {
            cause: error,
          },
        );
      }
      await wait(RETRY_MS);
    }
  }
}

/** The first page of the first context: the app's window, once WebView2 has created it. */
async function firstPage(browser: Browser, deadline: number): Promise<Page> {
  for (;;) {
    const page = browser.contexts()[0]?.pages()[0];
    if (page !== undefined) {
      return page;
    }
    remaining(deadline, "waiting for the app's page");
    await wait(RETRY_MS);
  }
}

/** Ends the process tree rooted at `pid`, by PID only. */
async function killTree(pid: number): Promise<void> {
  await promisify(execFile)("taskkill", ["/pid", String(pid), "/T", "/F"], {
    timeout: KILL_TIMEOUT_MS,
  });
}

/** Runs every step, even after one fails, then throws what failed. */
async function cleanUp(steps: readonly (() => unknown)[]): Promise<void> {
  const errors: unknown[] = [];
  for (const step of steps) {
    try {
      await step();
    } catch (error) {
      errors.push(error);
    }
  }
  if (errors.length > 0) {
    throw new AggregateError(errors, "cleaning up after the app failed");
  }
}

/** Whether `child` exits within `ms`. */
async function exited(child: ChildProcess, ms: number): Promise<boolean> {
  if (child.exitCode !== null || child.signalCode !== null) {
    return true;
  }
  return await new Promise<boolean>((resolve) => {
    const timer = setTimeout(() => {
      child.off("exit", onExit);
      resolve(false);
    }, ms);
    const onExit = (): void => {
      clearTimeout(timer);
      resolve(true);
    };
    child.once("exit", onExit);
  });
}

/** A launch the fixture owns, with what it needs to end it. */
interface Launched {
  app: App | undefined;
  child: ChildProcess;
  browser: Browser | undefined;
}

/** Starts the app on `scratch` as launch number `launch`. */
async function launch(scratch: Scratch, n: number, owned: Set<Launched>): Promise<App> {
  const userDataFolder = webviewFolder(scratch, n);
  const startedAt = performance.now();
  const deadline = startedAt + SETUP_TIMEOUT_MS;
  const child = spawn(appPath(), [], {
    env: { ...process.env, ...scratchEnv(scratch), WEBVIEW2_USER_DATA_FOLDER: userDataFolder },
    stdio: ["ignore", "pipe", "inherit"],
  });
  const launched: Launched = { app: undefined, child, browser: undefined };
  owned.add(launched);
  // Each line the app prints goes on to the test log with a prefix, so the pipe never fills.
  createInterface({ input: child.stdout }).on("line", (line) => {
    process.stdout.write(`[app ${n}] ${line}\n`);
  });
  const port = await readPort(child, userDataFolder, deadline);
  const { pid } = child;
  if (pid === undefined) {
    throw new Error(`the app has no PID: ${appPath()}`);
  }
  launched.browser = await connect(port, deadline);
  const page = await firstPage(launched.browser, deadline);
  launched.app = {
    page,
    browser: launched.browser,
    port,
    mode: mode(),
    pid,
    startedAt,
    userDataFolder,
    launch: n,
  };
  return launched.app;
}

/** Ends `launched`: disconnects, then closes or kills, and waits for the exit. */
async function end(launched: Launched, how: Stop): Promise<void> {
  const { child } = launched;
  await cleanUp([
    () => launched.browser?.close(),
    async () => {
      if (child.pid === undefined || (await exited(child, 0))) {
        return;
      }
      if (how === "close") {
        // No /T, no /F: WM_CLOSE to the app's window. A non-zero exit here only means it did not
        // take; the wait decides.
        await promisify(execFile)("taskkill", ["/pid", String(child.pid)], {
          timeout: KILL_TIMEOUT_MS,
        }).catch(() => undefined);
        if (await exited(child, CLOSE_TIMEOUT_MS)) {
          return;
        }
        const warning = `the app did not exit within ${CLOSE_TIMEOUT_MS} ms of closing its window; killed`;
        base.info().annotations.push({ type: "warning", description: warning });
        process.stdout.write(`::warning title=Close timed out::${warning}\n`);
      }
      await killTree(child.pid);
      await exited(child, KILL_TIMEOUT_MS);
    },
  ]);
}

/** Options a spec sets with `test.use`. */
export interface AppOptions {
  /** The fake_agent scenario `agents.claude` plays, or null for none (smoke, control). */
  scenario: string | null;
  /** The matter folder's files at the start, `/`-separated relative path to content. */
  matterFiles: Readonly<Record<string, string>>;
}

export const test = base.extend<AppOptions & { scratch: Scratch; apps: Apps; app: App }>({
  scenario: [null, { option: true }],
  matterFiles: [{}, { option: true }],

  scratch: async ({ scenario, matterFiles }, use) => {
    const scratch = makeScratch({ scenario, matterFiles }, () =>
      fakeAgentPath(process.env, appPath(), process.platform === "win32"),
    );
    try {
      await use(scratch);
    } finally {
      if (process.env["E2E_KEEP_SCRATCH"] === "1") {
        process.stdout.write(`scratch folders kept in ${scratch.root}\n`);
      } else {
        removeScratch(scratch);
      }
    }
  },

  apps: async ({ scratch }, use) => {
    const owned = new Set<Launched>();
    let launches = 0;
    const apps: Apps = {
      scratch,
      start: async () => {
        launches += 1;
        return await launch(scratch, launches, owned);
      },
      stop: async (app, how = "kill") => {
        const launched = [...owned].find((entry) => entry.app === app);
        if (launched === undefined) {
          throw new Error(`launch ${app.launch} is not running`);
        }
        owned.delete(launched);
        await end(launched, how);
      },
    };
    try {
      await use(apps);
    } finally {
      await cleanUp([...owned].map((launched) => () => end(launched, "kill")));
    }
  },

  app: async ({ apps }, use) => {
    await use(await apps.start());
  },
});
