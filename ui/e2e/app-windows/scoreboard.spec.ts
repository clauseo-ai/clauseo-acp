// The speed scoreboard on Windows (R13): fake_agent's `scoreboard` session played into the e2e
// build, once to warm up and three times measured, each play a fresh launch on its own scratch
// CLAUSEO_DATA_DIR and E2E_MATTER_DIR (one test per play, so the fixture gives each its folders).
// The page's perf API (`window.__clauseoPerf`, ui/app/src/perf/, loaded only in the e2e and marks
// builds) records the marks, the frames, chunk-to-paint and the keystrokes; this spec drives the
// session, reads memory with memory.ts and builds one RawRun (scoreboard/metrics.ts) per play. The
// last test prints the result as a table and writes scoreboard-windows.json.
//
// Slow (about a minute a play), so the default `pnpm test` leaves it out: playwright.config.ts
// keeps it out of the `app` project unless the command line names this file. Run it with
//   E2E_APP=<clauseo-desktop.exe> [SCOREBOARD_OUT=<folder>] pnpm -C ui/e2e test scoreboard.spec.ts
//
//   SCOREBOARD_OUT   folder that receives scoreboard-windows.json, and in runs/ each play's raw run
//                    and debug numbers (play-1-warmup.json, play-1-warmup.debug.json, play-2.json,
//                    ...); nothing is written when unset

import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, type Page } from "@playwright/test";

import {
  BYTES_PER_MB,
  MEASURED_RUNS,
  WARMUP_RUNS,
  parseRawRun,
  summarise,
  type RawRun,
} from "../scoreboard/metrics.ts";
import {
  PACE_ARGS,
  PROMPT,
  RUNS_DIR,
  SCENARIO,
  playDebug,
  playName,
  resultTable,
  startupBreakdown,
} from "../scoreboard/session.ts";
import { test, type App } from "./fixture.ts";
import { expectTurnEnded, openMatter } from "./flow.ts";
import { processTree, settle, treeMemory } from "./memory.ts";
import { window as appWindow, type Window as AppWindow } from "./selectors.ts";

test.use({ scenario: SCENARIO, agentArgs: PACE_ARGS });
// One play after another; a failed play skips the rest, since the result needs every run.
test.describe.configure({ mode: "serial" });

/**
 * The page's perf API (contract §5; ui/app/src/perf/ implements it). A copy of the shape: ui/e2e
 * imports nothing from ui/app.
 */
interface ClauseoPerf {
  marks(): { first_paint_ms: number | null; interactive_ms: number | null };
  startStream(): void;
  stopStream(): Promise<StreamSample>;
  startKeystrokes(): void;
  stopKeystrokes(): {
    latency_ms: number[] | null;
    handler_ms: number[] | null;
    /** Beyond the contract; optional so an older page still types. */
    entry_share?: number | null;
  };
  channelBytes(): Promise<number>;
}
interface StreamSample {
  first_token_on_screen_ms: number | null;
  chunk_to_paint_ms: number[];
  /** The older double-rAF measure of the same chunks, for the debug file only; optional so an
   * older page still runs. */
  chunk_to_paint_double_raf_ms?: number[];
  raf_timestamps_ms: number[];
  long_animation_frames_ms: number[] | null;
  react_commits_per_frame: number | null;
  react_commits_per_frame_max: number | null;
  answer_text_bytes: number;
}
declare global {
  interface Window {
    /** The init script's start time (contract §3); the debug file's start-up breakdown needs t0. */
    __CLAUSEO_PERF__?: { t0EpochMs: number };
    __clauseoPerf?: ClauseoPerf;
  }
}

/** How long the app may take to load the perf API and reach its `interactive` mark. */
const INTERACTIVE_TIMEOUT_MS = 90_000;
/**
 * The longest wait for idle memory to settle before it is read anyway. memory.ts's `settle` needs
 * at least 10 s of readings within 1%, so this is 10 to 20 s; the blank control waits up to 60 s,
 * but the room at idle has nothing left to load once interactive.
 */
const SETTLE_MAX_MS = 20_000;
/** Wheel steps up the transcript and back down, during the stream. */
const WHEEL_STEPS = 6;
const WHEEL_DELTA_PX = 400;

/** The private working set of the app's process tree, in MB (2^20 B), as control.spec.ts reads it. */
async function groupMemoryMb(app: App): Promise<number> {
  const memory = treeMemory(await processTree(app.pid, { workingSet: true }));
  if (memory.privateWorkingSetBytes === null) {
    throw new Error("a process of the app's tree has no private working set counter");
  }
  return memory.privateWorkingSetBytes / BYTES_PER_MB;
}

/** Waits for the next animation frame: paces the wheel steps by frames, not by a timer. */
async function nextFrame(page: Page): Promise<void> {
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => {
        requestAnimationFrame(() => {
          resolve();
        });
      }),
  );
}

/** Scrolls the transcript up by WHEEL_STEPS steps and back down, one step a frame. */
async function scrollUpAndDown(page: Page, w: AppWindow): Promise<void> {
  const box = await w.transcript.boundingBox();
  if (box === null) throw new Error("the transcript has no box to scroll");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  for (const delta of [-WHEEL_DELTA_PX, WHEEL_DELTA_PX]) {
    for (let i = 0; i < WHEEL_STEPS; i++) {
      await page.mouse.wheel(0, delta);
      await nextFrame(page);
    }
  }
}

/** SCOREBOARD_OUT, or null when unset. */
function outFolder(): string | null {
  const out = process.env["SCOREBOARD_OUT"];
  return out === undefined || out === "" ? null : out;
}

/** Writes `value` as `<SCOREBOARD_OUT>/runs/<name>.json`; nothing when SCOREBOARD_OUT is unset. */
function writeRunFile(name: string, value: unknown): void {
  const out = outFolder();
  if (out === null) return;
  const folder = join(out, RUNS_DIR);
  mkdirSync(folder, { recursive: true });
  writeFileSync(join(folder, `${name}.json`), `${JSON.stringify(value, null, 2)}\n`);
}

/** Play `n` of the session on a fresh launch; quits the app at the end. */
async function play(app: App, stop: (app: App) => Promise<void>, n: number): Promise<RawRun> {
  const { page } = app;
  const w = appWindow(page);

  const startToInteractive = await test.step("interactive", async () => {
    const handle = await page.waitForFunction(
      () => window.__clauseoPerf?.marks().interactive_ms ?? null,
      undefined,
      { timeout: INTERACTIVE_TIMEOUT_MS },
    );
    return (await handle.jsonValue()) as number;
  });

  // As the blank control reads it (control.spec.ts), so the two subtract: settled, one GC, then
  // the private working set of the whole tree.
  const cdp = await page.context().newCDPSession(page);
  const idleMemory = await test.step("idle memory", async () => {
    const pids = (await processTree(app.pid)).map((entry) => entry.pid);
    const settled = await settle(pids, SETTLE_MAX_MS);
    if (!settled.settled) {
      process.stdout.write(`[scoreboard] idle memory not settled after ${settled.ms} ms\n`);
    }
    await cdp.send("HeapProfiler.collectGarbage");
    return await groupMemoryMb(app);
  });

  // Read after the settle, so the page's load event has long passed.
  const startup = await page.evaluate(() => {
    const nav = performance.getEntriesByType("navigation")[0] as
      | PerformanceNavigationTiming
      | undefined;
    return {
      t0EpochMs: window.__CLAUSEO_PERF__?.t0EpochMs ?? null,
      timeOriginEpochMs: performance.timeOrigin,
      navigation:
        nav === undefined
          ? null
          : {
              fetchStart: nav.fetchStart,
              requestStart: nav.requestStart,
              responseStart: nav.responseStart,
              responseEnd: nav.responseEnd,
              domContentLoadedEventEnd: nav.domContentLoadedEventEnd,
              loadEventEnd: nav.loadEventEnd,
            },
      marks: window.__clauseoPerf!.marks(),
    };
  });
  if (startup.t0EpochMs === null) throw new Error("the page has no __CLAUSEO_PERF__.t0EpochMs");

  await openMatter(w);

  const keys = await test.step("type the prompt", async () => {
    await page.evaluate(() => window.__clauseoPerf!.startKeystrokes());
    await w.composer.pressSequentially(PROMPT);
    return await page.evaluate(() => window.__clauseoPerf!.stopKeystrokes());
  });

  const { sample, bytes, before, after } = await test.step("stream the session", async () => {
    const before = await page.evaluate(() => window.__clauseoPerf!.channelBytes());
    await page.evaluate(() => window.__clauseoPerf!.startStream());
    await w.send.click();
    await expect(w.agentMessages.first()).toBeVisible();
    await scrollUpAndDown(page, w);
    await expect(w.permissions).toHaveCount(1);
    await w.permissionButton(w.permissions.first(), "allow_once").click();
    await expectTurnEnded(w, "end_turn");
    const recorded = await page.evaluate(() => window.__clauseoPerf!.stopStream());
    const after = await page.evaluate(() => window.__clauseoPerf!.channelBytes());
    return { sample: recorded, bytes: after - before, before, after };
  });

  const afterSession = await test.step("memory after the session", async () => {
    await cdp.send("HeapProfiler.collectGarbage");
    await cdp.send("HeapProfiler.collectGarbage");
    return await groupMemoryMb(app);
  });
  await cdp.detach();

  const raw = {
    start_to_interactive_ms: startToInteractive,
    first_token_on_screen_ms: sample.first_token_on_screen_ms,
    chunk_to_paint_ms: sample.chunk_to_paint_ms,
    raf_timestamps_ms: sample.raf_timestamps_ms,
    long_animation_frames_ms: sample.long_animation_frames_ms,
    keystroke_latency_ms: keys.latency_ms,
    keystroke_handler_ms: keys.handler_ms,
    idle_memory_mb: idleMemory,
    memory_after_session_mb: afterSession,
    react_commits_per_frame: sample.react_commits_per_frame,
    ipc_bytes_per_text_byte: sample.answer_text_bytes > 0 ? bytes / sample.answer_text_bytes : null,
  } satisfies RawRun;
  // Written before the checks, so a run they refuse can still be read.
  writeRunFile(playName(n), raw);
  writeRunFile(
    `${playName(n)}.debug`,
    playDebug({
      bytesBefore: before,
      bytesAfter: after,
      answerTextBytes: sample.answer_text_bytes,
      raf: sample.raf_timestamps_ms,
      chunks: sample.chunk_to_paint_ms,
      doubleRafChunks: sample.chunk_to_paint_double_raf_ms ?? null,
      keystrokeLatency: keys.latency_ms,
      entryShare: keys.entry_share ?? null,
      startup: startupBreakdown({
        ...startup,
        t0EpochMs: startup.t0EpochMs,
        // The fixture's times are on this process's performance clock.
        spawnEpochMs: performance.timeOrigin + app.startedAt,
        connectedEpochMs: performance.timeOrigin + app.connectedAt,
      }),
    }),
  );
  const run = parseRawRun(raw);
  await stop(app);
  return run;
}

const measured: RawRun[] = [];

for (let n = 1; n <= WARMUP_RUNS + MEASURED_RUNS; n++) {
  const warmup = n <= WARMUP_RUNS;
  test(`play ${n} of ${WARMUP_RUNS + MEASURED_RUNS}${warmup ? " (warm-up)" : ""}`, async ({
    app,
    apps,
  }) => {
    const run = await play(app, (a) => apps.stop(a, "close"), n);
    if (!warmup) measured.push(run);
  });
}

test("the result: medians of the measured plays", () => {
  const result = summarise("windows", measured);
  process.stdout.write(resultTable(result));
  const out = outFolder();
  if (out !== null) {
    mkdirSync(out, { recursive: true });
    const file = join(out, "scoreboard-windows.json");
    writeFileSync(file, `${JSON.stringify(result, null, 2)}\n`);
    process.stdout.write(`[scoreboard] wrote ${file}\n`);
  }
});
