// @vitest-environment happy-dom
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { InfoTip } from "./info-tip.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

async function mount(node: ReactNode) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(node));
  return host;
}

afterEach(async () => {
  await act(async () => root?.unmount());
  document.body.replaceChildren();
  root = null;
  host = null;
});

const TEXT = "Anthropic lists Brave Search as its web search subprocessor.";
const button = () =>
  document.querySelector<HTMLButtonElement>('button[aria-label="More about Brave Search"]')!;
const popup = () =>
  [...document.querySelectorAll("[role=dialog]")].find((d) => d.textContent === TEXT);

async function tip() {
  await mount(<InfoTip label="More about Brave Search">{TEXT}</InfoTip>);
}

test("a button named by its label, the icon hidden, the text out of sight until asked", async () => {
  await tip();
  expect(button()).not.toBeNull();
  expect(button().querySelector("svg")?.getAttribute("aria-hidden")).toBe("true");
  expect(popup()).toBeUndefined();
});

test("a press opens it and shows the text; Escape closes it", async () => {
  await tip();
  await act(async () => button().click());
  expect(popup()).toBeDefined();
  expect(button().getAttribute("aria-expanded")).toBe("true");
  await act(async () => {
    document.activeElement?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    );
  });
  expect(button().getAttribute("aria-expanded")).toBe("false");
});

test("the keyboard opens it: a native button, which Enter and Space press, from focus", async () => {
  await tip();
  const b = button();
  // happy-dom doesn't turn Enter or Space into a click as a browser does for a native button: the press stands in
  expect(b.tagName).toBe("BUTTON");
  expect(b.type).toBe("button");
  b.focus();
  expect(document.activeElement).toBe(b);
  await act(async () => {
    b.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    b.click();
  });
  expect(popup()).toBeDefined();
  await act(async () => {
    document.activeElement?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    );
  });
  expect(b.getAttribute("aria-expanded")).toBe("false");
  expect(document.activeElement).toBe(b);
});
