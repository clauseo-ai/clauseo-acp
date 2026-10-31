// How the page is shown: one attribute per axis on <html>, which the tokens key on.
//   data-theme="day|night"      the light.
//   data-render="full|lite"     lite takes every shadow, transition and animation away, for remote desktops and
//                               software rendering.
//   data-motion="full|reduced"  reduced: nothing moves or scales. Lite implies it.
//   lang="en|hi"                the language. Hindi tokens apply through :lang(hi).
//   data-text-size="14..20"     the root font size, 16 by default. Every type size is in rem.
//   data-platform="mac|windows|linux"
//                               the computer's OS, and with it the window's chrome: the corner lights on the Mac, the
//                               caption buttons on Windows and Linux.
// This module only shows a display and lets the parts read it. Which display to show (the lawyer's settings, the
// computer's light and motion preferences, what the host reports about remote sessions) is the app's to decide.

import { useSyncExternalStore } from "react";
import { flushSync } from "react-dom";

export type Theme = "day" | "night";
export type Render = "full" | "lite";
export type Motion = "full" | "reduced";
export type Lang = "en" | "hi";
/** The computer's OS, which the host reports. It sets the window's chrome, where Linux takes Windows' caption buttons
 * at the right, where its own desktops put them, and the words that name the computer's keys and file manager. */
export type Platform = "mac" | "windows" | "linux";
export const TEXT_SIZES = [14, 15, 16, 17, 18, 19, 20] as const;
export type TextSize = (typeof TEXT_SIZES)[number];

export type Display = {
  theme: Theme;
  render: Render;
  motion: Motion;
  lang: Lang;
  textSize: TextSize;
  platform: Platform;
};

/** What the page shows before the app says otherwise. */
export const DEFAULT_DISPLAY: Display = {
  theme: "day",
  render: "full",
  motion: "full",
  lang: "en",
  textSize: 16,
  platform: "mac",
};

let current: Display = DEFAULT_DISPLAY;
const listeners = new Set<() => void>();

const emit = () => {
  for (const l of listeners) l();
};

/** Sets the attributes on <html> for a display. */
function setAttributes(d: Display): void {
  const root = document.documentElement;
  root.dataset["theme"] = d.theme;
  root.dataset["render"] = d.render;
  root.dataset["motion"] = d.motion;
  root.dataset["textSize"] = String(d.textSize);
  root.dataset["platform"] = d.platform;
  root.lang = d.lang;
}

const canTransition = (d: Display) =>
  d.motion === "full" &&
  d.render === "full" &&
  !document.hidden &&
  typeof document.startViewTransition === "function";

/** Moves the page to a display. The state lands first; a cross-fade only decorates it, and a timer, never an
 * animation's promise, settles what follows. Under reduced motion and in lite every change lands at once. */
export function showDisplay(next: Display): void {
  const prev = current;
  const update = () => {
    current = next;
    setAttributes(next);
    flushSync(emit);
  };
  const themeChanged = prev.theme !== next.theme;
  const langChanged = prev.lang !== next.lang;
  const moving = next.motion === "full" && next.render === "full" && !document.hidden;
  if ((themeChanged || langChanged) && canTransition(next)) {
    const root = document.documentElement;
    if (langChanged && !themeChanged) root.dataset["switching"] = "lang";
    let ran = false;
    const once = () => {
      if (!ran) {
        ran = true;
        update();
      }
    };
    try {
      const vt = document.startViewTransition(once);
      for (const p of [vt.ready, vt.finished, vt.updateCallbackDone]) p.catch(() => {});
    } catch {
      once();
    }
    setTimeout(() => {
      once();
      delete root.dataset["switching"];
    }, 420);
    return;
  }
  update();
  if (themeChanged && moving) {
    // no view transitions: every plane's colours ease together for the same 320 ms instead
    const root = document.documentElement;
    root.dataset["theming"] = "";
    setTimeout(() => delete root.dataset["theming"], 360);
  } else if (langChanged && moving) {
    // no view transitions: the words come up from 0.6 over 200 ms
    document.body.animate([{ opacity: 0.6 }, { opacity: 1 }], {
      duration: 200,
      easing: "cubic-bezier(0.16, 1, 0.3, 1)",
    });
  }
}

/* ---------- Devanagari metrics in WebKit ----------
   fonts.css re-centres Anek Devanagari with ascent and descent overrides; WebKit ignores them. A probe at 13 px on a
   22 px line should have its baseline 15.124 px down. More than 0.2 px off, <html> takes .no-metrics and the
   difference in em as --deva-nudge, and base.css moves the words in controls by it. */
function checkMetrics(): void {
  const box = document.createElement("div");
  box.className = "metrics-probe";
  box.setAttribute("aria-hidden", "true");
  box.lang = "hi";
  const letter = document.createElement("span");
  letter.textContent = "\u0915";
  const base = document.createElement("i");
  box.append(letter, base);
  document.body.append(box);
  const offset = base.getBoundingClientRect().top - box.getBoundingClientRect().top;
  box.remove();
  const nudge = (15.124 - offset) / 13;
  const off = offset > 0 && Math.abs(nudge * 13) > 0.2;
  const root = document.documentElement;
  root.classList.toggle("no-metrics", off);
  if (off) root.style.setProperty("--deva-nudge", `${nudge.toFixed(4)}em`);
  else root.style.removeProperty("--deva-nudge");
}

/** Shows the first display without a transition and starts the fonts. Call once, before the first render. */
export function startDisplay(first: Display): void {
  current = first;
  setAttributes(first);
  // Both scripts' faces load from the start, so a switch of language never waits on a font or reflows after one.
  const fonts = document.fonts;
  for (const w of [400, 600]) {
    fonts.load(`${w} 16px 'Anek Devanagari'`, "a\u0915").catch(() => {});
    fonts.load(`${w} 16px 'Anek Latin Hi'`, "a").catch(() => {});
  }
  Promise.all([
    fonts.load("400 100px 'Anek Devanagari'", "\u0915"),
    fonts.load("400 100px 'Anek Latin Hi'", "a"),
  ]).then(checkMetrics, () => {});
}

const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => listeners.delete(listener);
};
const snapshot = () => current;

/** What the page shows, live. */
export function useDisplay(): Display {
  return useSyncExternalStore(subscribe, snapshot, snapshot);
}

/** Whether motion may run now: full motion and full render. For WAAPI calls outside CSS. */
export const motionAllowed = () => current.motion === "full" && current.render === "full";
