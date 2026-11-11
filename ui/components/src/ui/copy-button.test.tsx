// @vitest-environment happy-dom
// Copy (D-026), the one control under an answer and on its code blocks: the copy icon, named and tipped Copy; a press
// puts the words on the clipboard, the tick stands in the icon's place for 2 s while a polite live region says
// Copied, then both go back.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, expect, test, vi } from "vitest";
import { IconCopy, IconTick } from "../icons/icons.tsx";
import { CopyButton } from "./copy-button.tsx";
import { TooltipProvider } from "./tooltip.tsx";

/** How long the tick stands: about 2 s, the time the answer's and the code block's Copy each held "Copied". */
const COPIED_MS = 2_000;

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
  Reflect.deleteProperty(navigator, "clipboard");
  vi.useRealTimers();
});

const clipboard = (write: () => Promise<void> = () => Promise.resolve()) => {
  const writeText = vi.fn(write);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  return writeText;
};

/** The paths an icon draws, to tell the copy icon from the tick. */
const paths = (markup: string) => [...markup.matchAll(/ d="([^"]+)"/g)].map((m) => m[1]);
const COPY_ICON = paths(renderToStaticMarkup(<IconCopy />));
const TICK_ICON = paths(renderToStaticMarkup(<IconTick />));

async function show(text: () => string) {
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <TooltipProvider>
        <CopyButton text={text} label="कॉपी करें" done="कॉपी हो गया" testId="copy" />
      </TooltipProvider>,
    ),
  );
  const button = host.querySelector<HTMLButtonElement>('[data-testid="copy"]')!;
  const status = host.querySelector('[role="status"]')!;
  const icon = () => paths(button.innerHTML);
  return { host, button, status, icon };
}

test("the copy icon, named in the room's words, a 28 px hit area at least, and a live region that waits empty", async () => {
  const { button, status, icon } = await show(() => "Section 138");
  expect(button.getAttribute("aria-label")).toBe("कॉपी करें");
  expect(button.textContent).toBe("");
  expect(icon()).toEqual(COPY_ICON);
  // the small icon button: h- and w-control-small, 32 px times the text size's k, 28 px at its smallest (k 0.875)
  expect(button.className).toContain("h-control-small");
  expect(button.className).toContain("w-control-small");
  expect(status.getAttribute("aria-live")).toBe("polite");
  expect(status.textContent).toBe("");
  // the live region stands outside the button, whose aria-label would hide it
  expect(button.contains(status)).toBe(false);
});

test("its tooltip says the same words", async () => {
  const { button } = await show(() => "Section 138");
  await act(async () => {
    button.focus();
    button.dispatchEvent(new FocusEvent("focus"));
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  expect(document.querySelector("[data-base-ui-portal] > [data-side]")?.textContent).toBe(
    "कॉपी करें",
  );
});

test("a press copies the words; the tick and Copied stand for about 2 s, then the copy icon is back", async () => {
  vi.useFakeTimers();
  const writeText = clipboard();
  const { button, status, icon } = await show(() => "Section 138");
  await act(async () => button.click());
  expect(writeText).toHaveBeenCalledWith("Section 138");
  expect(icon()).toEqual(TICK_ICON);
  expect(status.textContent).toBe("कॉपी हो गया");
  expect(button.getAttribute("aria-label")).toBe("कॉपी करें");
  await act(async () => vi.advanceTimersByTime(COPIED_MS - 1));
  expect(icon()).toEqual(TICK_ICON);
  await act(async () => vi.advanceTimersByTime(1));
  expect(icon()).toEqual(COPY_ICON);
  expect(status.textContent).toBe("");
});

test("a second press while the tick shows holds it a full 2 s from the second press", async () => {
  vi.useFakeTimers();
  clipboard();
  const { button, icon } = await show(() => "Section 138");
  await act(async () => button.click());
  await act(async () => vi.advanceTimersByTime(1_500));
  await act(async () => button.click());
  await act(async () => vi.advanceTimersByTime(1_500));
  expect(icon()).toEqual(TICK_ICON);
  await act(async () => vi.advanceTimersByTime(500));
  expect(icon()).toEqual(COPY_ICON);
});

test("nothing to copy, no clipboard, or a refused write: no tick and nothing said", async () => {
  const writeText = clipboard();
  const empty = await show(() => "");
  await act(async () => empty.button.click());
  expect(writeText).not.toHaveBeenCalled();
  expect(empty.icon()).toEqual(COPY_ICON);
  await act(async () => root?.unmount());
  document.body.replaceChildren();

  clipboard(() => Promise.reject(new Error("refused")));
  const refused = await show(() => "Section 138");
  await act(async () => refused.button.click());
  expect(refused.icon()).toEqual(COPY_ICON);
  expect(refused.status.textContent).toBe("");
  await act(async () => root?.unmount());
  document.body.replaceChildren();

  // happy-dom has a clipboard of its own, as a webview may not: none here
  Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true });
  const none = await show(() => "Section 138");
  await act(async () => none.button.click());
  expect(none.icon()).toEqual(COPY_ICON);
});
