// @vitest-environment happy-dom
// UD1, the brief's Performance: install progress (at most every 250 ms) must not regress react_commits_per_frame. Each
// AgentInstallProgress is one React commit, in the room (the composer's install line, read from the setup store in the
// same batch as the first run's own render: hold.ts) and on the first screen. Commits counted as the perf recorder
// counts them: every onCommitFiberRoot of a DevTools hook installed before react-dom loads.
import { act } from "react";
import { expect, test, vi } from "vitest";

const hook = vi.hoisted(() => {
  const state = { commits: 0 };
  (globalThis as { __REACT_DEVTOOLS_GLOBAL_HOOK__?: unknown }).__REACT_DEVTOOLS_GLOBAL_HOOK__ = {
    supportsFiber: true,
    isDisabled: false,
    renderers: new Map(),
    inject: () => 1,
    checkDCE: () => {},
    onScheduleFiberRoot: () => {},
    onCommitFiberUnmount: () => {},
    onPostCommitFiberRoot: () => {},
    onCommitFiberRoot: () => {
      state.commits += 1;
    },
  };
  return state;
});

import { A, CLAUDE, byId, install, mountFirstRun, signIn } from "./rig.tsx";
import type { FakeTransport } from "../../transport/fake-transport.ts";

/** The commits each of four progress events makes, each played in its own act as the channel delivers them. */
async function commitsPerEvent(t: FakeTransport): Promise<number[]> {
  const per: number[] = [];
  for (const received of [40, 80, 120, 160]) {
    const before = hook.commits;
    await act(async () => {
      t.setupEvent(install(CLAUDE, { downloading: { received: received * 1e6, total: 200e6 } }));
    });
    per.push(hook.commits - before);
  }
  return per;
}

test("one commit per install progress event in the room, the composer's install line included", async () => {
  const t = await mountFirstRun({
    matters: [A],
    first: [
      install(CLAUDE, { downloading: { received: 1, total: 200e6 } }),
      signIn(CLAUDE, { signed_out: {} }),
    ],
  });
  expect(byId("install-foot")?.closest("[data-composer]")).not.toBeNull();
  expect(await commitsPerEvent(t)).toEqual([1, 1, 1, 1]);
  expect(byId("install-foot")?.querySelector('[role="meter"]')?.getAttribute("aria-valuenow")).toBe(
    "80",
  );
});

test("one commit per install progress event on the first screen", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [install(CLAUDE, { downloading: { received: 1, total: 200e6 } })],
  });
  expect(byId("install-foot")).not.toBeNull();
  expect(await commitsPerEvent(t)).toEqual([1, 1, 1, 1]);
});
