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

test("zero timers and animation frames at rest: signed in and installed, with the first screen gone", async () => {
  const t = await mountFirstRun({
    first: [
      install(CLAUDE, { downloading: { received: 1, total: 2 } }),
      signIn(CLAUDE, { signed_out: {} }),
    ],
  });
  vi.useFakeTimers({
    toFake: ["setTimeout", "setInterval", "requestAnimationFrame", "requestIdleCallback"],
  });
  await act(async () => {
    t.setupEvent(install(CLAUDE, READY));
    t.setupEvent(signIn(CLAUDE, SIGNED_IN));
    await vi.runAllTimersAsync();
  });
  expect(byId("signin")).toBeNull();
  expect(vi.getTimerCount()).toBe(0);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5_000);
  });
  expect(vi.getTimerCount()).toBe(0);
});

test("zero timers at rest on the first screen, waiting for the host", async () => {
  await mountFirstRun({
    matters: [],
    first: [install(CLAUDE, READY), signIn(CLAUDE, { signed_out: {} })],
  });
  vi.useFakeTimers({
    toFake: ["setTimeout", "setInterval", "requestAnimationFrame", "requestIdleCallback"],
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5_000);
  });
  expect(vi.getTimerCount()).toBe(0);
});
