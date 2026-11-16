// limits.test.tsx's waits on a chunk inside an act, each bounded. Every one ends by WAIT_MS into the test, a second
// before its 5 s timeout. An act still awaiting when a test times out stays open, and React holds every later test's
// renders in its queue (no composer in any of them); bounded, a chunk slow to load (a machine under load) fails its own
// test, with the act closed.
import { act } from "react";

const WAIT_MS = 4_000;
let deadline = 0;

/** Starts the test's clock for its waits; call in beforeEach. */
export function startWaits() {
  deadline = performance.now() + WAIT_MS;
}

/** Awaits `wait` inside one act, no later than the test's deadline; past it, closes the act and fails the test. */
export async function bounded(what: string, wait: () => Promise<unknown>) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let late = false;
  const cutoff = new Promise<void>((resolve) => {
    timer = setTimeout(
      () => {
        late = true;
        resolve();
      },
      Math.max(0, deadline - performance.now()),
    );
  });
  try {
    await act(async () => {
      await Promise.race([wait(), cutoff]);
    });
  } finally {
    clearTimeout(timer);
  }
  if (late) throw new Error(`${what} did not settle within ${WAIT_MS} ms of the test's start`);
}
