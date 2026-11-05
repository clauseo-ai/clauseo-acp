// @vitest-environment happy-dom
// The Hindi words load only for a Hindi display (words-hi.ts, a lazy chunk). A page that starts in Hindi first renders
// once they are in, so its first frame is Hindi, never English; an English page renders at once and loads nothing. A
// switch to Hindi at runtime keeps the page in its language until they are in, then moves: no blank frame, and no
// English key under lang="hi". Every test here starts from a page that has not loaded them (forgetHindiForTests).
import { act, useLayoutEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import {
  DEFAULT_DISPLAY,
  showDisplay,
  startDisplay,
  whenWordsReady,
  type Display,
} from "./display.ts";
import { settled } from "./late.ts";
import { forgetHindiForTests, hindiLoaded, loadHindi } from "./words-core.ts";
import { useUiWords } from "./words-ui.ts";
import { useWords } from "./words.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const EN: Display = { ...DEFAULT_DISPLAY, motion: "reduced" };
const HI: Display = { ...EN, lang: "hi" };
const DEVA = /[\u0900-\u097F]/;
/** The English the probe shows, each of which the Hindi page must never show. */
const ENGLISH = ["Matters", "Pin to the top"];

/** Each commit: <html lang> and the words on the page, as the reader saw them. */
let frames: { lang: string; text: string }[] = [];
let container: HTMLElement;
let root: Root | null = null;

function Probe() {
  const w = useWords();
  const u = useUiWords();
  useLayoutEffect(() => {
    frames.push({ lang: document.documentElement.lang, text: container.textContent ?? "" });
  });
  return (
    <p>
      <span>{w.t("Matters")}</span> <span>{u.ui("Pin to the top")}</span>
    </p>
  );
}

const mount = () => {
  root = createRoot(container);
  act(() => root?.render(<Probe />));
};

beforeEach(() => {
  frames = [];
  container = document.createElement("div");
  document.body.append(container);
  forgetHindiForTests();
});

afterEach(async () => {
  act(() => root?.unmount());
  root = null;
  container.remove();
  // every other test file has the Hindi words in from the start (testing/setup.ts)
  await loadHindi();
  startDisplay(DEFAULT_DISPLAY);
});

test("a page that starts in Hindi first renders once the Hindi words are in, in Hindi: never English first", async () => {
  startDisplay(HI);
  // startDisplay set off the load before any render
  expect(hindiLoaded()).toBe(false);
  whenWordsReady(mount);
  expect(root).toBeNull();
  expect(container.textContent).toBe("");
  await act(settled);
  expect(hindiLoaded()).toBe(true);
  expect(root).not.toBeNull();
  expect(container.textContent).toBe("मामले सबसे ऊपर पिन करें");
  expect(frames.length).toBeGreaterThan(0);
  for (const frame of frames) {
    expect(frame.lang).toBe("hi");
    expect(frame.text).toMatch(DEVA);
    for (const english of ENGLISH) expect(frame.text).not.toContain(english);
  }
});

test("a page that starts in English renders at once and loads no Hindi", async () => {
  startDisplay(EN);
  whenWordsReady(mount);
  // mounted in the same task: no wait for English
  expect(root).not.toBeNull();
  expect(container.textContent).toBe("Matters Pin to the top");
  await act(settled);
  expect(hindiLoaded()).toBe(false);
  expect(frames).toEqual([{ lang: "en", text: "Matters Pin to the top" }]);
});

test("switching to Hindi at runtime keeps the page in English until the Hindi words are in, then shows Hindi", async () => {
  startDisplay(EN);
  whenWordsReady(mount);
  act(() => showDisplay(HI));
  // the load is on its way: the page is still English, whole
  expect(hindiLoaded()).toBe(false);
  expect(document.documentElement.lang).toBe("en");
  expect(container.textContent).toBe("Matters Pin to the top");
  await act(settled);
  expect(document.documentElement.lang).toBe("hi");
  expect(container.textContent).toBe("मामले सबसे ऊपर पिन करें");
  // no blank frame, and no English under lang="hi"
  expect(frames.at(-1)).toEqual({ lang: "hi", text: "मामले सबसे ऊपर पिन करें" });
  for (const frame of frames) {
    expect(frame.text).not.toBe("");
    if (frame.lang === "hi")
      for (const english of ENGLISH) expect(frame.text).not.toContain(english);
    else expect(frame.text).toBe("Matters Pin to the top");
  }
});

test("while the Hindi words load, the rest of the display moves at once, and a later switch back to English wins", async () => {
  startDisplay(EN);
  whenWordsReady(mount);
  act(() => showDisplay({ ...HI, theme: "night" }));
  expect(document.documentElement.dataset["theme"]).toBe("night");
  expect(document.documentElement.lang).toBe("en");
  act(() => showDisplay({ ...EN, theme: "night" }));
  await act(settled);
  // the load landed, but the page asked for English since
  expect(hindiLoaded()).toBe(true);
  expect(document.documentElement.lang).toBe("en");
  expect(container.textContent).toBe("Matters Pin to the top");
  // and a switch once the words are in moves at once
  act(() => showDisplay(HI));
  expect(document.documentElement.lang).toBe("hi");
  expect(container.textContent).toBe("मामले सबसे ऊपर पिन करें");
});
