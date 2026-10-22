// scratch: where a hot reload stalls on this machine. Dropped before review.
// Starts vite with vite:hmr debug output, opens the page in headless Chrome, edits App.tsx three
// times per write method (direct, then the probe's FileGuard rename) and logs, with ms since each edit, what vite printed, what the page logged, the websocket
// frames, and when #app-root showed the edit (60 s limit per edit).

import { spawn } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { watch } from "node:fs";
import { chromium } from "playwright-core";
import { appPaths, marked } from "./buildtime-lib.mjs";
import { FileGuard } from "./guard.mjs";

const APP_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const APP = resolve(APP_DIR, "src/App.tsx");
const TAG = '<div id="app-root"';
const t0 = performance.now();
let since = t0;
const log = (what) =>
  process.stdout.write(`[${(performance.now() - t0).toFixed(0)} ms | +${(performance.now() - since).toFixed(0)}] ${what}\n`);

const port = await new Promise((done) => {
  const s = createServer();
  s.listen(0, "127.0.0.1", () => {
    const { port: p } = s.address();
    s.close(() => done(p));
  });
});
const original = readFileSync(APP, "utf8");
const vite = spawn(process.execPath, [resolve(APP_DIR, "node_modules/vite/bin/vite.js"), "--port", String(port), "--strictPort"], {
  cwd: APP_DIR,
  env: { ...process.env, NO_COLOR: "1", DEBUG: "vite:hmr,vite:watcher" },
});
let ready;
const isReady = new Promise((r) => (ready = r));
for (const stream of [vite.stdout, vite.stderr]) {
  stream.on("data", (chunk) => {
    for (const line of chunk.toString("utf8").split(/\r?\n/).filter(Boolean)) {
      log(`vite: ${line.slice(0, 300)}`);
      if (/ready in/.test(line)) ready();
    }
  });
}
await isReady;
const browser = await chromium.launch({ headless: true, channel: "chrome" });
log(`chrome ${browser.version()}`);
const page = await browser.newPage();
page.on("console", (m) => log(`page console ${m.type()}: ${m.text().slice(0, 300)}`));
page.on("pageerror", (e) => log(`page error: ${e.message}`));
page.on("websocket", (ws) => {
  log(`page websocket open ${ws.url()}`);
  ws.on("framereceived", (f) => log(`ws <- ${String(f.payload).slice(0, 200)}`));
  ws.on("close", () => log("ws closed"));
});
await page.goto(`http://localhost:${port}/`);
await page.waitForSelector("#app-root", { state: "attached" });
log("page loaded, #app-root attached");
const srcWatch = watch(resolve(APP_DIR, "src"), (event, name) => log(`fs.watch ${event} ${name}`));
const results = { direct: [], guard: [] };
async function edit(method, n, write) {
  since = performance.now();
  write(n);
  log(`wrote edit ${n} (${method})`);
  try {
    await page.waitForFunction(
      (want) => document.querySelector("#app-root")?.getAttribute("data-buildtime") === want,
      String(n),
      { polling: "raf", timeout: 15_000 },
    );
    results[method].push(Math.round(performance.now() - since));
    log(`edit ${n} landed`);
  } catch (error) {
    results[method].push("miss");
    log(`edit ${n} did not land in 15 s: ${String(error).split("\n")[0]}`);
    log(`page #app-root: ${await page.evaluate(() => document.querySelector("#app-root")?.outerHTML)}`);
  }
}
let guard;
try {
  for (let n = 1; n <= 10; n += 1) {
    await edit("direct", n, (k) => writeFileSync(APP, original.replace(TAG, `${TAG} data-buildtime="${k}"`)));
  }
  writeFileSync(APP, original);
  await new Promise((r) => setTimeout(r, 1000));
  guard = new FileGuard(appPaths(resolve(APP_DIR, "../.."), "ui/app/src/App.tsx"));
  for (let n = 11; n <= 20; n += 1) {
    await edit("guard", n, (k) => guard.write(marked(guard.original.toString("utf8"), k)));
  }
  log(`RESULTS ${JSON.stringify(results)}`);
} finally {
  srcWatch.close();
  if (guard) guard.restore();
  writeFileSync(APP, original);
  await browser.close();
  vite.kill();
}
