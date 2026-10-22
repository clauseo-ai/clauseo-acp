// The real app on Windows under Playwright. The e2e build (`--features e2e`) opens WebView2's
// DevTools port and prints `E2E_CDP_PORT=<port>`; the fixture starts the app with a temporary
// WebView2 data folder, attaches with `connectOverCDP`, hands the test the app's page, then closes
// the connection, ends the app's whole process tree by its PID and deletes the folder.
//
//   E2E_APP   path to the e2e build of clauseo-desktop.exe (required)

import { execFileSync, spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { setTimeout as wait } from "node:timers/promises";
import { chromium, test as base, type Browser, type Page } from "@playwright/test";

/** How long the app may take to print its port, and WebView2 to accept a connection on it. */
const START_TIMEOUT_MS = 60_000;
/** The pause between two connection attempts while WebView2 opens its port. */
const RETRY_MS = 200;

/** The running app, as the test sees it. */
export interface App {
  /** The page of the app's one window. */
  page: Page;
  /** The CDP connection to the app's WebView2. */
  browser: Browser;
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

/** The port from the app's `E2E_CDP_PORT=<port>` line. Rejects if the app exits or stays silent. */
function readPort(child: ReturnType<typeof spawn>): Promise<number> {
  return new Promise((done, fail) => {
    if (child.stdout === null) {
      fail(new Error("the app was started without a stdout pipe"));
      return;
    }
    const lines = createInterface({ input: child.stdout });
    const timer = setTimeout(() => {
      lines.close();
      fail(new Error(`the app printed no E2E_CDP_PORT line within ${START_TIMEOUT_MS} ms`));
    }, START_TIMEOUT_MS);
    lines.on("line", (line) => {
      const match = /^E2E_CDP_PORT=(\d+)$/.exec(line.trim());
      if (match?.[1] !== undefined) {
        clearTimeout(timer);
        lines.close();
        done(Number(match[1]));
      }
    });
    child.once("exit", (code) => {
      clearTimeout(timer);
      fail(new Error(`the app exited with ${String(code)} before it printed E2E_CDP_PORT`));
    });
  });
}

/** Connects to WebView2 on `port`, retrying while it opens the port. */
async function connect(port: number): Promise<Browser> {
  const deadline = performance.now() + START_TIMEOUT_MS;
  for (;;) {
    try {
      return await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
    } catch (error) {
      if (performance.now() > deadline) {
        throw new Error(`WebView2 accepted no CDP connection on port ${port}`, { cause: error });
      }
      await wait(RETRY_MS);
    }
  }
}

/** The first page of the first context: the app's window, once WebView2 has created it. */
async function firstPage(browser: Browser): Promise<Page> {
  const deadline = performance.now() + START_TIMEOUT_MS;
  for (;;) {
    const page = browser.contexts()[0]?.pages()[0];
    if (page !== undefined) {
      return page;
    }
    if (performance.now() > deadline) {
      throw new Error("WebView2 has no page");
    }
    await wait(RETRY_MS);
  }
}

/** Ends the process tree rooted at `pid`, by PID only. */
function killTree(pid: number): void {
  execFileSync("taskkill", ["/pid", String(pid), "/T", "/F"], { stdio: "inherit" });
}

export const test = base.extend<{ app: App }>({
  app: async ({}, use) => {
    const userDataFolder = mkdtempSync(join(tmpdir(), "clauseo-e2e-"));
    const startedAt = performance.now();
    const child = spawn(appPath(), [], {
      env: { ...process.env, WEBVIEW2_USER_DATA_FOLDER: userDataFolder },
      stdio: ["ignore", "pipe", "inherit"],
    });
    const { pid } = child;
    let browser: Browser | undefined;
    try {
      if (pid === undefined) {
        throw new Error(`the app did not start: ${appPath()}`);
      }
      const port = await readPort(child);
      browser = await connect(port);
      const page = await firstPage(browser);
      await use({ page, browser, pid, startedAt, userDataFolder });
    } finally {
      await browser?.close();
      if (pid !== undefined && child.exitCode === null) {
        killTree(pid);
      }
      // WebView2's processes may hold the folder for a moment after they are ended.
      rmSync(userDataFolder, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
    }
  },
});
