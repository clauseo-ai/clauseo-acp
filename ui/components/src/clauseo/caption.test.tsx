// @vitest-environment happy-dom
// The caption buttons call the window through the handlers the app passes, and fall back to saying what they would
// do; the corner lights give way to macOS's own without moving.
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { CaptionButtons, CornerLights } from "./caption.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

async function mount(node: ReactNode) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(node));
}

async function rerender(node: ReactNode) {
  await act(async () => root?.render(node));
}

const buttons = () => [...(host?.querySelectorAll("button") ?? [])];
const maximise = () => host?.querySelector<HTMLButtonElement>('[data-caption="maximise"]') ?? null;
const press = async (b: HTMLButtonElement | null | undefined) => {
  await act(async () => b?.click());
};

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  vi.restoreAllMocks();
});

test("each press calls its own handler, and nothing is logged", async () => {
  const log = vi.spyOn(console, "info").mockImplementation(() => {});
  const onMinimise = vi.fn();
  const onToggleMaximise = vi.fn();
  const onClose = vi.fn();
  await mount(
    <CaptionButtons
      maximised={false}
      onMinimise={onMinimise}
      onToggleMaximise={onToggleMaximise}
      onClose={onClose}
    />,
  );
  const [min, max, close] = buttons();
  await press(min);
  expect([
    onMinimise.mock.calls.length,
    onToggleMaximise.mock.calls.length,
    onClose.mock.calls.length,
  ]).toEqual([1, 0, 0]);
  await press(max);
  expect([
    onMinimise.mock.calls.length,
    onToggleMaximise.mock.calls.length,
    onClose.mock.calls.length,
  ]).toEqual([1, 1, 0]);
  await press(close);
  expect([
    onMinimise.mock.calls.length,
    onToggleMaximise.mock.calls.length,
    onClose.mock.calls.length,
  ]).toEqual([1, 1, 1]);
  expect(log).not.toHaveBeenCalled();
});

test("the middle button follows maximised as given, not its own presses", async () => {
  const onToggleMaximise = vi.fn();
  await mount(<CaptionButtons maximised onToggleMaximise={onToggleMaximise} />);
  expect(maximise()?.getAttribute("aria-label")).toBe("Restore");
  await press(maximise());
  // the window hasn't said it changed, so the label stands
  expect(maximise()?.getAttribute("aria-label")).toBe("Restore");
  await rerender(<CaptionButtons maximised={false} onToggleMaximise={onToggleMaximise} />);
  expect(maximise()?.getAttribute("aria-label")).toBe("Maximise");
});

test("without handlers each press logs what it would do, and the middle button keeps its own state", async () => {
  const log = vi.spyOn(console, "info").mockImplementation(() => {});
  await mount(<CaptionButtons />);
  expect(maximise()?.getAttribute("aria-label")).toBe("Maximise");
  await press(maximise());
  expect(log).toHaveBeenLastCalledWith("Clauseo would maximise the window.");
  expect(maximise()?.getAttribute("aria-label")).toBe("Restore");
  await press(maximise());
  expect(log).toHaveBeenLastCalledWith("Clauseo would restore the window.");
  expect(maximise()?.getAttribute("aria-label")).toBe("Maximise");
  const [min, , close] = buttons();
  await press(min);
  expect(log).toHaveBeenLastCalledWith("Clauseo would minimise the window.");
  await press(close);
  expect(log).toHaveBeenLastCalledWith("Clauseo would close the window.");
});

test("the host finds the maximise button, and only it, by data-caption", async () => {
  await mount(<CaptionButtons />);
  expect(host?.querySelectorAll("[data-caption]").length).toBe(1);
  expect(buttons()[1]).toBe(maximise());
});

test("native corner lights keep their three circles and say macOS draws them", async () => {
  await mount(
    <>
      <CornerLights native />
      <CornerLights />
    </>,
  );
  const [native, drawn] = [...(host?.querySelectorAll(".room-lights") ?? [])];
  expect(native?.querySelectorAll("i").length).toBe(3);
  expect(native?.hasAttribute("data-native")).toBe(true);
  expect(native?.hasAttribute("data-tauri-drag-region")).toBe(true);
  expect(drawn?.querySelectorAll("i").length).toBe(3);
  expect(drawn?.hasAttribute("data-native")).toBe(false);
});
