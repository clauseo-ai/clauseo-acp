// The words of the controls in ui/: menus, dialogs, toasts, settings rows and the samples they hold.
// As in words.ts the English is the key, the exact string on screen, and a key with slots fills them in either
// language. The Hindi is a specialist's draft, waiting for a Hindi-speaking lawyer: the formal आप, the danda,
// Western digits, English legal names and file names in Latin script.
import { createElement, type ReactNode } from "react";
import { clock, t as roomWords, type Key } from "./words.ts";
import { useDisplay, type Platform } from "./display.ts";
import { frame, type FrameKey } from "./words-frame.ts";
import { Mixed } from "./mixed.tsx";
import { wordsOf, type Slots } from "./words-core.ts";
import type { HI } from "./words-ui.hi.ts";

/** Every new interface string, in English. */
export type UiKey = keyof typeof HI;

/** A new string in the language, its slots filled. */
export const ui = wordsOf("ui");

/** The OS from what the webview reports: navigator.userAgentData.platform where there is one, else
 * navigator.platform or the user agent. */
export function osOf(platform: string): Platform {
  const p = platform.toLowerCase();
  if (p.includes("win")) return "windows";
  if (p.includes("linux") || p.includes("x11")) return "linux";
  return "mac";
}

/** The computer's own file manager: "Show in Finder" on the Mac, "Show in File Explorer" on Windows, and on Linux,
 * whose file manager has no one name, "Show in folder". */
export function revealKey(os: Platform): UiKey {
  return os === "windows"
    ? "Show in File Explorer"
    : os === "linux"
      ? "Show in folder"
      : "Show in Finder";
}

/** Words to render: a string with every English run inside Hindi set in lang="en". */
export const say = (s: string): ReactNode => createElement(Mixed, null, s);

/** The new words of the page's language, bound, with the room's words and the frame's beside them: const w =
 * useUiWords(); w.ui("Rename"), w.t("{n} more", { n: 3 }), w.f("Pinned"), w.clock("3:58 pm"). ui, t and f give
 * strings, for attributes such as aria-label; say gives the words to render, with their English runs marked. */
export function useUiWords() {
  const { lang } = useDisplay();
  return {
    lang,
    ui: (key: UiKey, slots?: Slots) => ui(lang, key, slots),
    t: (key: Key, slots?: Slots) => roomWords(lang, key, slots),
    f: (key: FrameKey, slots?: Slots) => frame(lang, key, slots),
    clock: (time: string) => clock(lang, time),
    say,
  };
}
