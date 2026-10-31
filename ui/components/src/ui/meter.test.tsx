// @vitest-environment happy-dom
// The plan meter is a control: it carries the title bar's drag region only when the title bar asks for it, on its four
// elements, and without the ask no element of it drags the window.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { PlanMeter } from "./meter.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

async function mount(node: React.ReactNode) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(node));
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const dragged = () => host?.querySelectorAll("[data-tauri-drag-region]") ?? [];

test("without dragRegion, no element of the meter drags the window", async () => {
  await mount(<PlanMeter name="Plan" value={30} valueText="30%" status="Plenty left" />);
  expect(host?.querySelector("[data-state]")).not.toBeNull();
  expect(dragged().length).toBe(0);
});

test("with dragRegion, the meter's four elements drag the window", async () => {
  await mount(<PlanMeter name="Plan" value={30} valueText="30%" status="Plenty left" dragRegion />);
  expect(dragged().length).toBe(4);
});
