// @vitest-environment happy-dom
// The model or effort menu (config-menu.tsx): opened by keys, the focus lands on the value ticked now, so Enter keeps it
// and the arrows move from it; with no value ticked, on the first. A pointer's open moves no focus into it.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { ConfigMenuTrigger } from "./config-menu.tsx";
import type { ConfigMenu } from "./status-line.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  for (const n of document.body.querySelectorAll("[data-base-ui-portal]")) n.remove();
});

const settle = () =>
  act(async () => {
    for (let i = 0; i < 4; i += 1) await new Promise((resolve) => setTimeout(resolve, 20));
  });

const menuOf = (value: string, picked: string[] = []): ConfigMenu => ({
  id: "model",
  label: "Model",
  value,
  options: [
    { value: "opus", name: "Opus", description: "For the hardest questions" },
    { value: "sonnet", name: "Sonnet" },
    { value: "haiku", name: "Haiku" },
  ],
  onValueChange: (next) => picked.push(next),
});

async function mount(menu: ConfigMenu) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<ConfigMenuTrigger menu={menu} />));
  return host.querySelector<HTMLButtonElement>('[data-testid="pick-model"]')!;
}

const focused = () => (document.activeElement as HTMLElement | null)?.textContent ?? "";
const key = (target: Element, k: string) =>
  act(async () => {
    target.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }));
  });

test("opened by keys, the focus lands on the ticked value, and the arrows move from it", async () => {
  const picked: string[] = [];
  const trigger = await mount(menuOf("sonnet", picked));
  await act(async () => trigger.focus());
  // Enter or Space on a button is its click, with no press behind it
  await act(async () => trigger.click());
  await settle();
  const active = document.activeElement as HTMLElement;
  expect(active.getAttribute("role")).toBe("menuitemradio");
  expect(active.getAttribute("aria-checked")).toBe("true");
  expect(focused()).toBe("Sonnet");
  expect(active.hasAttribute("data-highlighted")).toBe(true);
  await key(active, "ArrowDown");
  await settle();
  expect(focused()).toBe("Haiku");
  expect(picked).toEqual([]);
});

test("the Effort menu as the Model one: the ticked value takes the focus", async () => {
  const effort: ConfigMenu = {
    id: "effort",
    label: "Effort",
    value: "high",
    options: [
      { value: "low", name: "Low" },
      { value: "medium", name: "Medium" },
      { value: "high", name: "High" },
    ],
    onValueChange: () => {},
  };
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<ConfigMenuTrigger menu={effort} />));
  const trigger = host.querySelector<HTMLButtonElement>('[data-testid="pick-effort"]')!;
  await act(async () => trigger.focus());
  await act(async () => trigger.click());
  await settle();
  expect(focused()).toBe("High");
});

test("with no value ticked, keys open it on the first value", async () => {
  // a value the agent no longer lists: nothing in the menu is ticked
  const trigger = await mount(menuOf("retired"));
  await act(async () => trigger.focus());
  await act(async () => trigger.click());
  await settle();
  expect(document.querySelector('[aria-checked="true"]')).toBeNull();
  expect(focused()).toContain("Opus");
});

test("a pointer's open moves no focus to a value", async () => {
  const trigger = await mount(menuOf("sonnet"));
  await act(async () => {
    trigger.dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 1 }));
  });
  await settle();
  expect(document.querySelector('[role="menu"]')).not.toBeNull();
  expect((document.activeElement as HTMLElement | null)?.getAttribute("role")).not.toBe(
    "menuitemradio",
  );
});
