// @vitest-environment happy-dom
// One animation frame callback runs every store's flush, so two stores that change in the same frame (the thread's
// view and the matters' rows, private issue #253) notify React in one task and the frame commits once.
import {
  act,
  createElement,
  Profiler,
  useSyncExternalStore,
  type ProfilerOnRenderCallback,
} from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { frameBatch } from "./batch.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  vi.useRealTimers();
});

/** A store that applies its pending value once a frame through `frameBatch`, as the view and matters stores do. */
function tinyStore() {
  let value = 0;
  let pending: number | null = null;
  let cancel: (() => void) | null = null;
  const listeners = new Set<() => void>();
  return {
    set(next: number) {
      pending = next;
      cancel ??= frameBatch(() => {
        cancel = null;
        value = pending!;
        for (const l of listeners) l();
      });
    },
    cancel: () => cancel?.(),
    get: () => value,
    subscribe: (l: () => void) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
  };
}

test("two stores changed in one frame commit once, in one animation frame callback", async () => {
  vi.useFakeTimers({
    toFake: ["requestAnimationFrame", "cancelAnimationFrame", "setTimeout", "clearTimeout"],
  });
  const a = tinyStore();
  const b = tinyStore();
  const Read = () =>
    `${useSyncExternalStore(a.subscribe, a.get)} ${useSyncExternalStore(b.subscribe, b.get)}`;
  let commits = 0;
  const count: ProfilerOnRenderCallback = () => (commits += 1);
  const host = document.createElement("div");
  const root = createRoot(host);
  await act(async () =>
    root.render(createElement(Profiler, { id: "r", onRender: count }, createElement(Read))),
  );
  commits = 0;
  a.set(1);
  b.set(2);
  const timers = vi.getTimerCount();
  await act(async () => vi.advanceTimersToNextFrame());
  expect(host.textContent).toBe("1 2");
  expect(commits).toBe(1);
  expect(timers).toBe(2); // one animation frame and its 100 ms fallback, for both stores
  expect(vi.getTimerCount()).toBe(0);
  act(() => root.unmount());
});

test("a cancelled flush drops only its own store's; the last cancel stops the frame", async () => {
  vi.useFakeTimers({
    toFake: ["requestAnimationFrame", "cancelAnimationFrame", "setTimeout", "clearTimeout"],
  });
  const ran: string[] = [];
  const cancelA = frameBatch(() => ran.push("a"));
  const cancelB = frameBatch(() => ran.push("b"));
  cancelA();
  vi.advanceTimersToNextFrame();
  expect(ran).toEqual(["b"]);
  frameBatch(() => ran.push("c"))();
  cancelB();
  expect(vi.getTimerCount()).toBe(0);
});

test("a flush that throws doesn't stop the others", async () => {
  vi.useFakeTimers({
    toFake: ["requestAnimationFrame", "cancelAnimationFrame", "setTimeout", "clearTimeout"],
  });
  const ran: string[] = [];
  frameBatch(() => {
    throw new Error("first");
  });
  frameBatch(() => ran.push("second"));
  expect(() => vi.advanceTimersToNextFrame()).toThrow("first");
  expect(ran).toEqual(["second"]);
});
