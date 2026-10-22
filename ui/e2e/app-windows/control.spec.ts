// The blank app's numbers, the control for every memory and start-up budget: time to #app-root,
// the process tree's private bytes right then, and once its memory has settled and a GC has run,
// its private bytes and private working set and the page's JS heap and DOM nodes.
//
//   E2E_RESULTS   where to write the numbers as JSON (required)

import { writeFileSync } from "node:fs";
import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { processTree, settle, treeMemory } from "./memory.ts";

/** How long the tree's memory may take to settle before it is sampled anyway. */
const SETTLE_MAX_MS = 30_000;

function resultsPath(): string {
  const path = process.env["E2E_RESULTS"];
  if (path === undefined || path === "") {
    throw new Error("E2E_RESULTS is not set; set it to the JSON file the numbers go to");
  }
  return path;
}

test("the blank app's numbers", async ({ app }) => {
  const out = resultsPath();
  await expect(app.page.locator("#app-root")).toBeAttached();
  // From the start of the process to the first time the harness saw the element, so it includes
  // the CDP connection.
  const msToRoot = performance.now() - app.startedAt;
  const atRoot = treeMemory(await processTree(app.pid)).privateBytes;

  const idle = await settle(app.pid, SETTLE_MAX_MS);
  const cdp = await app.page.context().newCDPSession(app.page);
  await cdp.send("HeapProfiler.collectGarbage");
  await cdp.send("Performance.enable");
  const { metrics } = await cdp.send("Performance.getMetrics");
  const metric = (name: string): number | null =>
    metrics.find((entry) => entry.name === name)?.value ?? null;
  const tree = await processTree(app.pid, { workingSet: true });
  const memory = treeMemory(tree);

  const results = {
    mode: app.mode,
    jsHeapUsedBytes: metric("JSHeapUsedSize"),
    domNodes: metric("Nodes"),
    // Settled, after the GC.
    groupPrivateBytes: memory.privateBytes,
    groupPrivateWorkingSetBytes: memory.privateWorkingSetBytes,
    msToRoot: Math.round(msToRoot),
    settled: idle.settled,
    msToSettle: idle.ms,
    atRoot: { groupPrivateBytes: atRoot },
    webview2: app.browser.version(),
    userAgent: await app.page.evaluate(() => navigator.userAgent),
    processes: tree,
  };
  writeFileSync(out, `${JSON.stringify(results, null, 2)}\n`);
  expect(results.jsHeapUsedBytes).not.toBeNull();
  expect(results.domNodes).not.toBeNull();
});
