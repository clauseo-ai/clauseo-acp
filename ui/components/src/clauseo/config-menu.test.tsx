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

const menuOf = (value: string, picked: string[] = [], scope = "thread:a"): ConfigMenu => ({
  id: "model",
  scope,
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

test("a menu on no value: the trigger says the menu's own name, never an id, and no row is ticked", async () => {
  const trigger = await mount(menuOf(""));
  expect(trigger.textContent).toBe("Model");
  expect(trigger.getAttribute("aria-label")).toBe("Model");
  await act(async () => trigger.click());
  await settle();
  expect(document.querySelector('[aria-checked="true"]')).toBeNull();
});

test("a polite live region says the value as the trigger names it each time it changes, never as it first shows", async () => {
  const trigger = await mount(menuOf("sonnet"));
  const live = () => host?.querySelector<HTMLElement>('[aria-live="polite"]');
  expect(live()?.textContent).toBe("");
  expect(live()?.classList.contains("sr-only")).toBe(true);
  // a pick, as the room shows it at once
  await act(async () => root?.render(<ConfigMenuTrigger menu={menuOf("haiku")} />));
  expect(live()?.textContent).toBe("Model: Haiku");
  expect(trigger.getAttribute("aria-label")).toBe("Model: Haiku");
  // the host moving it back
  await act(async () => root?.render(<ConfigMenuTrigger menu={menuOf("sonnet")} />));
  expect(live()?.textContent).toBe("Model: Sonnet");
});

test("another conversation's value comes in silently: the live region says nothing and empties", async () => {
  await mount(menuOf("sonnet"));
  const live = () => host?.querySelector<HTMLElement>('[aria-live="polite"]');
  await act(async () => root?.render(<ConfigMenuTrigger menu={menuOf("haiku")} />));
  expect(live()?.textContent).toBe("Model: Haiku");
  // another conversation opened, its model another
  await act(async () => root?.render(<ConfigMenuTrigger menu={menuOf("opus", [], "thread:b")} />));
  expect(live()?.textContent).toBe("");
  const trigger = host?.querySelector<HTMLButtonElement>('[data-testid="pick-model"]');
  expect(trigger?.getAttribute("aria-label")).toBe("Model: Opus");
  // back in the first, at its own value: still nothing
  await act(async () => root?.render(<ConfigMenuTrigger menu={menuOf("haiku", [], "thread:a")} />));
  expect(live()?.textContent).toBe("");
  // a pick there speaks again
  await act(async () =>
    root?.render(<ConfigMenuTrigger menu={menuOf("sonnet", [], "thread:a")} />),
  );
  expect(live()?.textContent).toBe("Model: Sonnet");
});
