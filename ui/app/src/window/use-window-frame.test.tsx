// @vitest-environment happy-dom
// The caption buttons' window state follows the window: read on mount and after a resize, once per frame, never after
// unmount; each press reaches its control, and a failing call is logged, not thrown.
import { act, useEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { WindowControls } from "../transport/window-controls.ts";
import { useWindowFrame, type WindowFrame } from "./use-window-frame.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** A window that answers isMaximised from `state.maximised` and counts every call. */
function fakeWindow(maximised: boolean) {
  const state = { maximised };
  const controls = {
    minimise: vi.fn(async () => {}),
    toggleMaximise: vi.fn(async () => {}),
    close: vi.fn(async () => {}),
    isMaximised: vi.fn(async () => state.maximised),
  } satisfies WindowControls;
  return { state, controls };
}

/** Animation frames run only when the test says. */
let frames = new Map<number, FrameRequestCallback>();
let nextFrame = 1;
const runFrames = async () => {
  const due = [...frames.values()];
  frames = new Map();
  await act(async () => {
    for (const f of due) f(0);
  });
};

let host: HTMLDivElement | null = null;
let root: Root | null = null;
let frame: WindowFrame | null = null;

function Probe({ controls }: { controls: WindowControls }) {
  const latest = useWindowFrame(controls);
  useEffect(() => {
    frame = latest;
  });
  return null;
}

async function mount(controls: WindowControls) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<Probe controls={controls} />));
}

const resize = async () => {
  await act(async () => window.dispatchEvent(new Event("resize")));
};

beforeEach(() => {
  vi.stubGlobal("requestAnimationFrame", (f: FrameRequestCallback) => {
    const id = nextFrame++;
    frames.set(id, f);
    return id;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
});

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  frame = null;
  frames = new Map();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

test("the first state is the window's own", async () => {
  const { controls } = fakeWindow(true);
  await mount(controls);
  expect(controls.isMaximised).toHaveBeenCalledTimes(1);
  expect(frame?.maximised).toBe(true);
});

test("resizes re-read the state, once per animation frame", async () => {
  const { state, controls } = fakeWindow(false);
  await mount(controls);
  expect(frame?.maximised).toBe(false);
  state.maximised = true;
  await resize();
  await resize();
  expect(controls.isMaximised).toHaveBeenCalledTimes(1);
  await runFrames();
  expect(controls.isMaximised).toHaveBeenCalledTimes(2);
  expect(frame?.maximised).toBe(true);
});

test("each press calls its own control", async () => {
  const { controls } = fakeWindow(false);
  await mount(controls);
  await act(async () => frame?.onMinimise());
  expect(controls.minimise).toHaveBeenCalledTimes(1);
  await act(async () => frame?.onToggleMaximise());
  expect(controls.toggleMaximise).toHaveBeenCalledTimes(1);
  await act(async () => frame?.onClose());
  expect(controls.close).toHaveBeenCalledTimes(1);
  expect(controls.minimise).toHaveBeenCalledTimes(1);
  expect(controls.toggleMaximise).toHaveBeenCalledTimes(1);
});

test("a failing call is logged and never thrown", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const { controls } = fakeWindow(false);
  controls.isMaximised.mockRejectedValueOnce(new Error("no window"));
  controls.close.mockImplementationOnce(() => {
    throw new Error("gone");
  });
  await mount(controls);
  await act(async () => frame?.onClose());
  expect(frame?.maximised).toBe(false);
  expect(warn.mock.calls.map((c) => String(c[0]))).toEqual([
    "window isMaximised failed: Error: no window",
    "window close failed: Error: gone",
  ]);
});

test("after unmount a resize reads nothing, and a late reply is dropped", async () => {
  const { controls } = fakeWindow(false);
  let reply: (m: boolean) => void = () => {};
  controls.isMaximised.mockImplementationOnce(
    () => new Promise<boolean>((resolve) => (reply = resolve)),
  );
  const errors = vi.spyOn(console, "error");
  await mount(controls);
  await resize();
  await act(async () => root?.unmount());
  root = null;
  expect(frames.size).toBe(0);
  await resize();
  await runFrames();
  await act(async () => reply(true));
  expect(controls.isMaximised).toHaveBeenCalledTimes(1);
  expect(errors).not.toHaveBeenCalled();
});

test("a slow reply overtaken by a later read's does not stand", async () => {
  const replies: ((m: boolean) => void)[] = [];
  const controls = {
    ...fakeWindow(false).controls,
    isMaximised: vi.fn(() => new Promise<boolean>((resolve) => replies.push(resolve))),
  } satisfies WindowControls;
  await mount(controls);
  await resize();
  await runFrames();
  expect(replies).toHaveLength(2);
  // The later read answers first, then the first read's stale answer arrives.
  await act(async () => replies[1]?.(true));
  await act(async () => replies[0]?.(false));
  expect(frame?.maximised).toBe(true);
});
