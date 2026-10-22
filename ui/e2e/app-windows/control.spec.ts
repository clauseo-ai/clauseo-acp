// The blank app's numbers, the control for every memory and start-up budget: time to #app-root,
// the process tree's commit right then, and once its memory has settled and a GC has run, its
// private working set (the headline, which budgets use) and commit and the page's JS heap and DOM
// nodes.
//
//   E2E_RESULTS   where to write the numbers as JSON (required)

import { writeFileSync } from "node:fs";
import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { processTree, settle, treeMemory } from "./memory.ts";

/** How long the tree's memory may take to settle before it is sampled anyway. */
const SETTLE_MAX_MS = 60_000;
/**
 * A start slower than this gets a warning on the run page. The final runs took 1.6 to 2.2 s, and
 * the wait for #app-root allows 90 s, so without the warning a stall would pass unseen.
 */
const SLOW_START_MS = 10_000;

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
  const msToRoot = Math.round(performance.now() - app.startedAt);
  if (msToRoot > SLOW_START_MS) {
    const warning = `#app-root took ${msToRoot} ms in ${app.mode} mode, over ${SLOW_START_MS} ms`;
    test.info().annotations.push({ type: "warning", description: warning });
    // The job log shows the test's output line by line, so GitHub reads this as a workflow command.
    process.stdout.write(`::warning title=Slow start::${warning}\n`);
  }
  // The first snapshot after #app-root, without the working-set counters: those add seconds (on
  // windows-11-arm, WMI takes 10 to 20 s of this snapshot already).
  const rootTree = await processTree(app.pid);
  const atRoot = treeMemory(rootTree);

  const idle = await settle(
    rootTree.map((entry) => entry.pid),
    SETTLE_MAX_MS,
  );
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
    // Settled, after the GC. The private working set is the group metric budgets use; commit is a
    // second series, 1.5 to 2.5 times larger on GitHub's runners.
    groupPrivateWorkingSetBytes: memory.privateWorkingSetBytes,
    groupCommitBytes: memory.commitBytes,
    jsHeapUsedBytes: metric("JSHeapUsedSize"),
    domNodes: metric("Nodes"),
    msToRoot,
    atRoot: { groupCommitBytes: atRoot.commitBytes },
    settle: idle,
    webview2: app.browser.version(),
    userAgent: await app.page.evaluate(() => navigator.userAgent),
    processes: tree,
  };
  writeFileSync(out, `${JSON.stringify(results, null, 2)}\n`);
  expect(results.groupPrivateWorkingSetBytes).not.toBeNull();
  expect(results.jsHeapUsedBytes).not.toBeNull();
  expect(results.domNodes).not.toBeNull();
});
