// @vitest-environment happy-dom
// One animation frame callback runs every store's flush, so two stores that change in the same frame (the thread's
// view and the matters' rows, private issue #253) notify React in one task and the frame commits once.
import {
  createElement,
  Profiler,
  useSyncExternalStore,
  type ProfilerOnRenderCallback,
} from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { frameBatch } from "./batch.ts";

afterEach(() => {
  vi.useRealTimers();
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
});

/** Waits out `n` of the page's animation frames, each its own task. */
async function frames(n: number): Promise<void> {
  for (let i = 0; i < n; i++)
    await new Promise<void>((done) => requestAnimationFrame(() => done()));
}

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
  // Outside act, with the page's own frames and React's own scheduling: a microtask checkpoint runs after each
  // animation frame callback, as in a browser, so two stores flushing in two callbacks would commit twice.
  const env = globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean };
  env.IS_REACT_ACT_ENVIRONMENT = false;
  const a = tinyStore();
  const b = tinyStore();
  const Read = () =>
    `${useSyncExternalStore(a.subscribe, a.get)} ${useSyncExternalStore(b.subscribe, b.get)}`;
  let commits = 0;
  const count: ProfilerOnRenderCallback = () => (commits += 1);
  const host = document.createElement("div");
  const root = createRoot(host);
  root.render(createElement(Profiler, { id: "r", onRender: count }, createElement(Read)));
  await frames(3);
  expect(host.textContent).toBe("0 0");
  commits = 0;
  a.set(1);
  b.set(2);
  await frames(3);
  expect(host.textContent).toBe("1 2");
  expect(commits).toBe(1);
  root.unmount();
});

test("both stores' flushes wait for one animation frame and its fallback", () => {
  vi.useFakeTimers({
    toFake: ["requestAnimationFrame", "cancelAnimationFrame", "setTimeout", "clearTimeout"],
  });
  const ran: string[] = [];
  frameBatch(() => ran.push("a"));
  frameBatch(() => ran.push("b"));
  expect(vi.getTimerCount()).toBe(2);
  vi.advanceTimersToNextFrame();
  expect(ran).toEqual(["a", "b"]);
  expect(vi.getTimerCount()).toBe(0);
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

test("a flush that schedules again during the run goes to the next frame", () => {
  vi.useFakeTimers({
    toFake: ["requestAnimationFrame", "cancelAnimationFrame", "setTimeout", "clearTimeout"],
  });
  const ran: string[] = [];
  frameBatch(() => {
    ran.push("first");
    frameBatch(() => ran.push("again"));
  });
  vi.advanceTimersToNextFrame();
  expect(ran).toEqual(["first"]);
  expect(vi.getTimerCount()).toBe(2);
  vi.advanceTimersToNextFrame();
  expect(ran).toEqual(["first", "again"]);
});

test("frameBatch schedules as usual after a flush threw", () => {
  vi.useFakeTimers({
    toFake: ["requestAnimationFrame", "cancelAnimationFrame", "setTimeout", "clearTimeout"],
  });
  frameBatch(() => {
    throw new Error("boom");
  });
  expect(() => vi.advanceTimersToNextFrame()).toThrow("boom");
  const ran: string[] = [];
  frameBatch(() => ran.push("next"));
  expect(vi.getTimerCount()).toBe(2);
  vi.advanceTimersToNextFrame();
  expect(ran).toEqual(["next"]);
});
