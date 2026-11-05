// UD1, Done-when 1: the Safe workspace step is skipped until UC merges (decision 2 of the lead's brief), the case-law
// card's slot draws nothing until R24 merges, and the first run leaves no timer or animation frame at rest (G1b).
// @vitest-environment happy-dom
import { act } from "react";
import { expect, test, vi } from "vitest";
import { CaseLawSlot } from "./case-law.tsx";
import {
  CLAUDE,
  READY,
  SIGNED_IN,
  byId,
  install,
  mountFirstRun,
  play,
  settle,
  signIn,
  text,
} from "./rig.tsx";
import { workspaceStep } from "./workspace.ts";

test("the Safe workspace step is skipped before UC merges: none is needed, and after the first sign-in the composer takes the question", async () => {
  expect(workspaceStep()).toBeNull();
  const t = await mountFirstRun({
    first: [install(CLAUDE, READY), signIn(CLAUDE, { signed_out: {} })],
  });
  await play(t, signIn(CLAUDE, SIGNED_IN));
  expect(byId("signin")).toBeNull();
  expect(byId<HTMLTextAreaElement>("composer")?.hidden).toBe(false);
  expect(text()).not.toMatch(/Safe workspace|Windows asks once/i);
});

test("the case-law card's slot draws nothing before R24 merges", () => {
  expect(CaseLawSlot()).toBeNull();
});

/** The page's timers and animation frames, watched from before the room mounts: each live handle (a timeout or frame
 * not yet run, an interval not cleared) and how many were ever started. `wait` waits on a timer the watch doesn't
 * count. */
function watchTimers() {
  const g = globalThis as unknown as Record<string, (...args: unknown[]) => unknown>;
  const live = new Set<unknown>();
  let started = 0;
  const realTimeout = g["setTimeout"]!;
  const watch = (start: string, stop: string, once: boolean) => {
    const begin = g[start];
    const end = g[stop];
    if (begin === undefined || end === undefined) return;
    vi.spyOn(g, start).mockImplementation((run: unknown, ...rest: unknown[]) => {
      started += 1;
      const id: unknown = begin(
        (...args: unknown[]) => {
          if (once) live.delete(id);
          (run as (...a: unknown[]) => void)(...args);
        },
        ...rest,
      );
      live.add(id);
      return id;
    });
    vi.spyOn(g, stop).mockImplementation((id: unknown) => {
      live.delete(id);
      return end(id);
    });
  };
  watch("setTimeout", "clearTimeout", true);
  watch("setInterval", "clearInterval", false);
  watch("requestAnimationFrame", "cancelAnimationFrame", true);
  watch("requestIdleCallback", "cancelIdleCallback", true);
  return {
    live: () => live.size,
    started: () => started,
    wait: (ms: number) => new Promise((resolve) => realTimeout(resolve, ms)),
  };
}

/** No timer or frame left live at rest, and none started over a further wait. */
async function expectRest(timers: ReturnType<typeof watchTimers>) {
  await settle();
  expect(timers.live()).toBe(0);
  const before = timers.started();
  await act(async () => timers.wait(300));
  expect(timers.started() - before).toBe(0);
  expect(timers.live()).toBe(0);
}

test("zero timers and animation frames at rest in the room: signed in and installed, watched from before the mount", async () => {
  const timers = watchTimers();
  const t = await mountFirstRun({
    first: [
      install(CLAUDE, { downloading: { received: 1, total: 2 } }),
      signIn(CLAUDE, { signed_out: {} }),
    ],
  });
  await play(t, install(CLAUDE, "installing"), install(CLAUDE, READY), signIn(CLAUDE, SIGNED_IN));
  expect(byId("signin")).toBeNull();
  await expectRest(timers);
});

test("zero timers and animation frames at rest on the first screen, waiting for the host, watched from before the mount", async () => {
  const timers = watchTimers();
  await mountFirstRun({
    matters: [],
    first: [install(CLAUDE, READY), signIn(CLAUDE, { signed_out: {} })],
  });
  expect(byId("counts-switch")).not.toBeNull();
  await expectRest(timers);
});
