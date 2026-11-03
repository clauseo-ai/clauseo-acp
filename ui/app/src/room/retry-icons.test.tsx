// @vitest-environment happy-dom
// The retries outside the working line carry the retry icon before their words: a break's Restart and Try again, and
// Try again when Word holds a file. Hidden from assistive technology; the word names each button.
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { IconTryAgain } from "@clauseo/components/icons/icons.tsx";
import { iconIn } from "@clauseo/components/icons/icon-in.tsx";
import { RestartButton, RetryButton } from "./break-buttons.tsx";
import { WordLock } from "./word/word-lock.tsx";

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
  host?.remove();
  root = null;
});

test.each([
  ["Restart", <RestartButton key="restart" lang="en" onRestart={() => {}} />],
  ["Try again", <RetryButton key="retry" lang="en" onRetry={() => {}} />],
  [
    "Try again",
    <WordLock
      key="word"
      file="papers/Reply.docx"
      code="matter.file_locked"
      pending={false}
      onRetry={() => {}}
    />,
  ],
])("%s leads with the retry icon, hidden, and its word names it", async (word, node) => {
  const el = await mount(node);
  const button = el.querySelector("button");
  const svg = iconIn(button, IconTryAgain, 16);
  expect(svg?.getAttribute("aria-hidden")).toBe("true");
  expect(button?.firstElementChild).toBe(svg);
  expect(button?.textContent).toBe(word);
});
