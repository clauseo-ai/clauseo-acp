// U16b: Snooze and Done's later chunk is warmed once after the rail mounts: at idle where the page has an idle
// callback, and at mount where it has none (WebKit's WKWebView), as the thread chunk is; never by a timer.
// @vitest-environment happy-dom
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { railView } from "./rail-lists.ts";
import { mount, NOW, unmount } from "./rail-snooze-rig.ts";
import { preloadSnooze } from "./snooze-load.ts";

vi.mock("./snooze-load.ts", async (original) => {
  const real = await original<typeof import("./snooze-load.ts")>();
  return { ...real, preloadSnooze: vi.fn(real.preloadSnooze) };
});

beforeEach(() => {
  railView.set("matters");
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
  vi.mocked(preloadSnooze).mockClear();
});

afterEach(async () => {
  await unmount();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

test("with no idle callback (WKWebView), the chunk is warmed once, at mount", async () => {
  vi.stubGlobal("requestIdleCallback", undefined);
  await mount();
  // no pointer and no focus came to the rail: the one ask is the mount's
  expect(preloadSnooze).toHaveBeenCalledTimes(1);
});

test("with an idle callback, nothing at mount; once, when it fires", async () => {
  const asked: (() => void)[] = [];
  vi.stubGlobal(
    "requestIdleCallback",
    vi.fn((run: () => void) => asked.push(run)),
  );
  vi.stubGlobal("cancelIdleCallback", vi.fn());
  await mount();
  expect(preloadSnooze).not.toHaveBeenCalled();
  await act(async () => asked[0]?.());
  expect(preloadSnooze).toHaveBeenCalledTimes(1);
  expect(asked).toHaveLength(1);
});
