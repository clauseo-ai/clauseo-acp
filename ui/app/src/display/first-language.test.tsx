// @vitest-environment happy-dom
// The first paint is in the language the app will settle on. The desktop sets window.__CLAUSEO_UI_LANGUAGE__ before
// any page script (crates/desktop/src/ui_language.rs's init_script) from what the installer and the app record; the
// display store starts in it, over localStorage's cache and the default, and the host's settings still win once they
// arrive. A Hindi install's welcome is Hindi from its first frame, never the English one first.
import {
  DEFAULT_DISPLAY,
  showDisplay,
  startDisplay,
  whenWordsReady,
  type Display,
} from "@clauseo/components/lib/display.ts";
import { settled as lateSettled } from "@clauseo/components/lib/late.ts";
import { forgetHindiForTests } from "@clauseo/components/lib/words-core.ts";
import { Welcome } from "@clauseo/components/clauseo/first-launch/welcome.tsx";
import { act, useLayoutEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import {
  createDisplayStore,
  DEFAULT_UI,
  DISPLAY_KEY,
  type DisplayEnv,
} from "../room/display-store.ts";
import type { Settings, SettingsHost } from "../room/host-next.ts";
import { injectedLanguage } from "./connect.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** A host whose first read answers when the test says. */
function laterHost() {
  let answer: (settings: Settings) => void = () => {};
  const host: SettingsHost = {
    getSettings: () => new Promise<Settings>((resolve) => (answer = resolve)),
    setUiSettings: () => Promise.resolve(),
    setToolSettings: () => Promise.resolve(),
    onSettingsChanged: () => () => {},
  };
  const answerWith = (language: "en" | "hi") =>
    answer({ ui: { ...DEFAULT_UI, language }, locked: [] });
  return { host, answerWith };
}

function harness(over: Partial<DisplayEnv>, stored: string | null = null) {
  const items = new Map<string, string>();
  if (stored !== null) items.set(DISPLAY_KEY, stored);
  const started: Display[] = [];
  const shown: Display[] = [];
  const store = createDisplayStore({
    storage: {
      getItem: (key) => items.get(key) ?? null,
      setItem: (key, value) => void items.set(key, value),
    },
    matchMedia: null,
    host: null,
    start: (d) => started.push(d),
    show: (d) => shown.push(d),
    ...over,
  });
  return { store, started, shown };
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  delete window.__CLAUSEO_UI_LANGUAGE__;
});

test("injectedLanguage: the desktop's known language, null for none or anything else", () => {
  expect(injectedLanguage()).toBeNull();
  window.__CLAUSEO_UI_LANGUAGE__ = "hi";
  expect(injectedLanguage()).toBe("hi");
  window.__CLAUSEO_UI_LANGUAGE__ = "en";
  expect(injectedLanguage()).toBe("en");
  for (const odd of ["fr", "HI", "", 1, null, { lang: "hi" }]) {
    window.__CLAUSEO_UI_LANGUAGE__ = odd;
    expect(injectedLanguage()).toBeNull();
  }
});

test("the first display takes the desktop's language over localStorage's cache and the default", () => {
  expect(harness({ firstLanguage: "hi" }).started[0]?.lang).toBe("hi");
  expect(
    harness({ firstLanguage: "hi" }, JSON.stringify({ language: "en", theme: "dark" })).started[0],
  ).toMatchObject({ lang: "hi", theme: "night" });
  expect(
    harness({ firstLanguage: "en" }, JSON.stringify({ language: "hi" })).started[0]?.lang,
  ).toBe("en");
});

test("without the desktop's language, or with an unknown one, the first display is today's", () => {
  expect(harness({}).started[0]?.lang).toBe("en");
  expect(
    harness({ firstLanguage: null }, JSON.stringify({ language: "hi" })).started[0]?.lang,
  ).toBe("hi");
  const odd = { firstLanguage: "fr" } as unknown as Partial<DisplayEnv>;
  expect(harness(odd).started[0]?.lang).toBe("en");
});

test("the host's settings override the desktop's language once they arrive", async () => {
  const later = laterHost();
  const { store, started, shown } = harness({ firstLanguage: "hi", host: later.host });
  expect(started[0]?.lang).toBe("hi");
  later.answerWith("en");
  await tick();
  expect(store.ui().language).toBe("en");
  expect(shown.at(-1)?.lang).toBe("en");
});

test("the host agreeing with the desktop's language moves nothing", async () => {
  const later = laterHost();
  const { shown } = harness({ firstLanguage: "hi", host: later.host });
  later.answerWith("hi");
  await tick();
  expect(shown).toEqual([]);
});

/* ---------- the welcome's first render ---------- */

let container: HTMLElement;
let root: Root | null = null;
let frames: { lang: string; text: string }[] = [];

function Recorded() {
  useLayoutEffect(() => {
    frames.push({ lang: document.documentElement.lang, text: container.textContent ?? "" });
  });
  return (
    <Welcome
      name="Claude"
      install={null}
      ready={false}
      counts={{ on: true, locked: false, onChange: () => {} }}
      onOpenFolder={() => {}}
      onNewMatter={() => {}}
    />
  );
}

beforeEach(() => {
  frames = [];
  container = document.createElement("div");
  document.body.append(container);
  forgetHindiForTests();
  document.documentElement.lang = "en";
});

afterEach(async () => {
  act(() => root?.unmount());
  root = null;
  container.remove();
  delete window.__CLAUSEO_UI_LANGUAGE__;
  await lateSettled();
});

test("with the desktop's hi, the welcome's first render is Hindi, with no English frame before it", async () => {
  window.__CLAUSEO_UI_LANGUAGE__ = "hi";
  const later = laterHost();
  createDisplayStore({
    storage: null,
    matchMedia: null,
    host: later.host,
    start: startDisplay,
    show: showDisplay,
    firstLanguage: injectedLanguage(),
  });
  expect(document.documentElement.lang).toBe("hi");
  await new Promise<void>((resolve) =>
    whenWordsReady(() => {
      root = createRoot(container);
      act(() => root?.render(<Recorded />));
      resolve();
    }),
  );
  // the host's own answer, Hindi, lands after the first frame and moves nothing
  later.answerWith("hi");
  await tick();
  expect(frames.length).toBeGreaterThan(0);
  for (const frame of frames) {
    expect(frame.lang).toBe("hi");
    expect(frame.text).not.toContain("Clauseo works on your matters");
    expect(frame.text).toContain("Clauseo आपके मामलों पर");
  }
  // the module's display is reset for the next file
  startDisplay({ ...DEFAULT_DISPLAY });
});
