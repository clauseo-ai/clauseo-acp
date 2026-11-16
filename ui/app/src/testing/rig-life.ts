// A mounted rig's acts, so a test that times out fails alone. The body of a test that timed out runs on, its act()
// scope open, and the next test's act() calls overlap it: their renders never commit, and every later test in the
// file finds nothing to click (versions-lock.test.tsx: 1 timeout, then 12 of those). A rig made with rigLife runs its
// acts through it. Once its test has ended (its unmount, or this file's afterEach for a rig the test never got back),
// the rig takes no new act, waits a bounded time for the one in flight, and unmounts. first-run/rig.tsx does the same
// for its mount.
import { act } from "react";
import { afterEach } from "vitest";
import { HOOK_TIMEOUT_MS } from "./timeouts.ts";

/** How long an ending rig waits for an act in flight: long enough for a cold chunk load on a loaded machine, short
 * enough that a test whose act hangs costs its 15 s and this, not more; under the hook timeout, so the hook reports. */
export const OVERRUN_WAIT_MS = 10_000;

if (OVERRUN_WAIT_MS >= HOOK_TIMEOUT_MS)
  throw new Error("rig-life: OVERRUN_WAIT_MS must stay under the hook timeout");

/** The real setTimeout, held before a test can fake it, so the wait above always ends. */
const realSetTimeout = globalThis.setTimeout;
const realClearTimeout = globalThis.clearTimeout;
const noop = () => {};

export type RigLife = {
  /** React's act, refused once the rig's test has ended; an async one is kept until its scope closes. */
  act: typeof act;
  /** Ends the rig: no new act, the one in flight waited for (bounded), then `teardown`, once. */
  end: () => Promise<void>;
};

const living = new Set<RigLife>();

/** A rig's life; `teardown` unmounts it (React's own act, not the rig's). */
export function rigLife(teardown: () => Promise<void>): RigLife {
  let ended = false;
  let ending: Promise<void> | null = null;
  const busy = new Set<Promise<unknown>>();
  const life: RigLife = {
    // React's own overloads: a sync callback's act returns nothing to await, an async one's a promise
    act: ((callback: () => unknown) => {
      if (ended) throw new Error("the rig's test has ended: no more acts");
      let isAsync = false;
      const scope = act(() => {
        const result = callback();
        isAsync = result instanceof Promise;
        return result;
      });
      // a sync act has flushed by now; nothing of it to wait for
      if (!isAsync) return scope;
      const held = Promise.resolve(scope);
      busy.add(held);
      const done = () => void busy.delete(held);
      held.then(done, done);
      return held;
    }) as typeof act,
    end: () => {
      ending ??= (async () => {
        ended = true;
        living.delete(life);
        if (busy.size > 0) {
          let timer: ReturnType<typeof setTimeout> | undefined;
          const bound = new Promise<void>((resolve) => {
            timer = realSetTimeout(resolve, OVERRUN_WAIT_MS);
          });
          await Promise.race([Promise.allSettled(busy).then(noop), bound]);
          realClearTimeout(timer);
        }
        await teardown();
      })();
      return ending;
    },
  };
  living.add(life);
  return life;
}

// a rig whose test timed out before the test got it back (its `h = await mount…()` never assigned) ends here
afterEach(async () => {
  for (const life of living) await life.end();
});
