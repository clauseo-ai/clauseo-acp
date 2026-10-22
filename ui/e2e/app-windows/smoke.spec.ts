// The blank app starts, renders #app-root, and reports the control's numbers.
//
//   E2E_RESULTS   where to write the numbers as JSON (required)
//   E2E_MODE      a label for the run, written into the JSON ("normal" or "disable-gpu")

import { existsSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { processTree } from "./memory.ts";

function resultsPath(): string {
  const path = process.env["E2E_RESULTS"];
  if (path === undefined || path === "") {
    throw new Error("E2E_RESULTS is not set; set it to the JSON file the numbers go to");
  }
  return path;
}

test("the blank app renders #app-root", async ({ app }) => {
  const out = resultsPath();
  await expect(app.page.locator("#app-root")).toBeAttached();
  // From the start of the process to the first time the harness saw the element, so it includes
  // the CDP connection.
  const msToRoot = performance.now() - app.startedAt;

  const cdp = await app.page.context().newCDPSession(app.page);
  await cdp.send("HeapProfiler.collectGarbage");
  await cdp.send("Performance.enable");
  const { metrics } = await cdp.send("Performance.getMetrics");
  const metric = (name: string): number | null =>
    metrics.find((entry) => entry.name === name)?.value ?? null;
  const tree = processTree(app.pid);

  const results = {
    mode: process.env["E2E_MODE"] ?? "normal",
    jsHeapUsedBytes: metric("JSHeapUsedSize"),
    domNodes: metric("Nodes"),
    groupPrivateBytes: tree.reduce((sum, entry) => sum + entry.privateBytes, 0),
    msToRoot: Math.round(msToRoot),
    webview2: app.browser.version(),
    userAgent: await app.page.evaluate(() => navigator.userAgent),
    // WebView2 keeps its profile in <data folder>/EBWebView; false means it ignored the variable.
    tempDataFolderUsed: existsSync(join(app.userDataFolder, "EBWebView")),
    processes: tree,
  };
  writeFileSync(out, `${JSON.stringify(results, null, 2)}\n`);
  expect(results.jsHeapUsedBytes).not.toBeNull();
  expect(results.domNodes).not.toBeNull();
});
