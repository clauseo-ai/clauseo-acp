// @vitest-environment happy-dom
// The room's start as main.tsx runs it, for a lawyer whose last display was Hindi: connectRoom reads the stored
// settings before the first render and sets off the lazy Hindi words, and whenWordsReady mounts the room once they are
// in. Its first commit is Hindi: the rail's New matter never shows its English first.
import { DEFAULT_DISPLAY, startDisplay, whenWordsReady } from "@clauseo/components/lib/display.ts";
import { settled } from "@clauseo/components/lib/late.ts";
import { forgetHindiForTests, hindiLoaded, loadHindi } from "@clauseo/components/lib/words-core.ts";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { App } from "../App.tsx";
import { NullTransport } from "../transport/host-transport.ts";
import { DISPLAY_KEY } from "./display-store.ts";
import { connectRoom } from "./entry.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;

afterEach(async () => {
  act(() => root?.unmount());
  root = null;
  localStorage.removeItem(DISPLAY_KEY);
  await loadHindi();
  startDisplay(DEFAULT_DISPLAY);
});

test("a room whose stored language is Hindi first renders in Hindi, once the Hindi words are in", async () => {
  localStorage.setItem(DISPLAY_KEY, JSON.stringify({ language: "hi" }));
  forgetHindiForTests();
  const container = document.createElement("div");
  document.body.append(container);
  // every commit's text, from the first
  const commits: string[] = [];
  const seen = new MutationObserver(() => commits.push(container.textContent ?? ""));
  seen.observe(container, { childList: true, subtree: true, characterData: true });
  const transport = new NullTransport();
  connectRoom(transport);
  expect(document.documentElement.lang).toBe("hi");
  whenWordsReady(() => {
    root = createRoot(container);
    act(() => root?.render(<App transport={transport} />));
  });
  // nothing renders while the words load
  expect(root).toBeNull();
  expect(container.textContent).toBe("");
  await act(settled);
  expect(hindiLoaded()).toBe(true);
  expect(root).not.toBeNull();
  seen.disconnect();
  // the first commit with words in it (the mount commits inside the outer act)
  const first = commits.find((text) => text !== "") ?? "";
  expect(first).toContain("नया मामला");
  for (const text of commits) expect(text).not.toContain("New matter");
  container.remove();
});
