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

// inside the toolbar's plan button the meter takes role="none", which may carry no value (axe's aria-allowed-attr), and
// the button named by its words has its visible text, words apart, inside its name (label-content-name-mismatch)
test("inside a button, the meter carries no value attributes and its name and line stand words apart", async () => {
  await mount(
    <PlanMeter name="ChatGPT plan" value={98} valueText="98%" status="Plenty left" inButton />,
  );
  const none = host?.querySelector('[role="none"]');
  expect(none).not.toBeNull();
  for (const name of ["aria-valuenow", "aria-valuemin", "aria-valuemax", "aria-valuetext"])
    expect(none?.hasAttribute(name), name).toBe(false);
  expect(host?.textContent).toBe("ChatGPT plan Plenty left");
});

test("standing alone, the meter keeps its value for assistive technology", async () => {
  await mount(<PlanMeter name="Plan" value={30} valueText="30%" status="" />);
  const meter = host?.querySelector('[role="meter"]');
  expect(meter?.getAttribute("aria-valuenow")).toBe("30");
  expect(meter?.getAttribute("aria-valuetext")).toBe("30%");
  expect(host?.textContent?.replace(/x$/, "")).toBe("Plan");
});
