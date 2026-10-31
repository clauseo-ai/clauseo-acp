// The real app on Windows under Playwright. The e2e build (`--features e2e`) opens WebView2's
// DevTools port on a port the browser picks, which it writes to `EBWebView/DevToolsActivePort` in
// its data folder; the fixture starts the app with a temporary WebView2 data folder, reads the port
// from that file, attaches with `connectOverCDP`, hands the test the app's page, then closes the
// connection, ends the app's whole process tree by its PID and deletes the folder.
//
//   E2E_APP           path to the e2e build of clauseo-desktop.exe (required)
//   E2E_DISABLE_GPU   `1` runs the webview with --disable-gpu; the app reads it too

import { execFile, spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as wait } from "node:timers/promises";
import { promisify } from "node:util";
import { chromium, test as base, type Browser, type Page } from "@playwright/test";

/** The time from starting the app to having its page: the port file, connecting, the page. */
const SETUP_TIMEOUT_MS = 60_000;
/** How long `taskkill` may take. */
const KILL_TIMEOUT_MS = 30_000;
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
  /** The temporary WebView2 data folder (`WEBVIEW2_USER_DATA_FOLDER`). */
  userDataFolder: string;
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
  child.once("error", (error) => {
    failure = new Error(`the app did not start: ${appPath()}`, { cause: error });
  });
  child.once("exit", (code) => {
    failure ??= new Error(`the app exited with ${String(code)} before WebView2 wrote ${file}`);
  });
  for (;;) {
    if (failure !== undefined) {
      throw failure;
    }
    let text = "";
    try {
      text = readFileSync(file, "utf8");
    } catch {
      // Not written yet.
    }
    // The line ends with a newline once the port is whole; a file caught mid-write waits a round.
    const match = /^(\d+)\r?\n/.exec(text);
    if (match?.[1] !== undefined) {
      return Number(match[1]);
    }
    if (performance.now() + RETRY_MS > deadline) {
      throw new Error(
        `WebView2 wrote no port to ${file} within the ${SETUP_TIMEOUT_MS} ms setup time;` +
          ` EBWebView holds: ${entries(join(userDataFolder, "EBWebView"))}`,
      );
    }
    await wait(RETRY_MS);
  }
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

export const test = base.extend<{ app: App }>({
  app: async ({}, use) => {
    const userDataFolder = mkdtempSync(join(tmpdir(), "clauseo-e2e-"));
    const startedAt = performance.now();
    const deadline = startedAt + SETUP_TIMEOUT_MS;
    const child = spawn(appPath(), [], {
      env: { ...process.env, WEBVIEW2_USER_DATA_FOLDER: userDataFolder },
      stdio: ["ignore", "inherit", "inherit"],
    });
    let browser: Browser | undefined;
    try {
      const port = await readPort(child, userDataFolder, deadline);
      const { pid } = child;
      if (pid === undefined) {
        throw new Error(`the app has no PID: ${appPath()}`);
      }
      browser = await connect(port, deadline);
      const page = await firstPage(browser, deadline);
      await use({ page, browser, port, mode: mode(), pid, startedAt, userDataFolder });
    } finally {
      await cleanUp([
        () => browser?.close(),
        () =>
          child.pid !== undefined && child.exitCode === null ? killTree(child.pid) : undefined,
        // WebView2's processes may hold the folder for a moment after they are ended.
        () =>
          rmSync(userDataFolder, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }),
      ]);
    }
  },
});
