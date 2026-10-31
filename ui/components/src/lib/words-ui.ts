// The words of the controls in ui/: menus, dialogs, toasts, settings rows and the samples they hold.
// As in words.ts the English is the key, the exact string on screen, and a key with slots fills them in either
// language. The Hindi is a specialist's draft, waiting for a Hindi-speaking lawyer: the formal आप, the danda,
// Western digits, English legal names and file names in Latin script.
import { createElement, type ReactNode } from "react";
import { clock, t as roomWords, type Key } from "./words.ts";
import { useDisplay } from "./display.ts";
import { frame, type FrameKey } from "./words-frame.ts";
import { Mixed } from "./mixed.tsx";
import { wordsOf, type Slots } from "./words-core.ts";

const HI = {
  // a matter's actions, from its row in the list
  "Actions for {name}": "{name} के विकल्प",
  "Pin to the top": "सबसे ऊपर पिन करें",
  "Show in Finder": "Finder में दिखाएँ",
  "Show in File Explorer": "File Explorer में दिखाएँ",
  "Show in folder": "फ़ोल्डर में दिखाएँ",
  "Remove from the list": "सूची से हटाएँ",
  "The folder and its papers stay where they are.": "फ़ोल्डर और उसके दस्तावेज़ अपनी जगह पर रहते हैं।",
  // the plan to ask with
  "Plan to ask with": "प्रश्न किस प्लान से पूछें",
  // restoring a version
  Restore: "पुनर्स्थापित करें",
  Cancel: "रद्द करें",
  // undoing over a later edit
  "Undo anyway puts back the version from before Clauseo's edit. The changes made since stay in Versions.":
    "फिर भी पूर्ववत करने पर Clauseo के संपादन से पहले का संस्करण लौट आता है। उसके बाद के बदलाव संस्करणों में बने रहते हैं।",
  "Undo anyway": "फिर भी पूर्ववत करें",
  "Keep current": "मौजूदा संस्करण रखें",
  // settings, in sections
  Settings: "सेटिंग्स",
  Matters: "मामले",
  // a confirmation
  Close: "बंद करें",
  Notifications: "सूचनाएँ",
  // the matter's folder and its note
  "Locate folder": "फ़ोल्डर ढूँढें",
} as const;

/** Every new interface string, in English. */
export type UiKey = keyof typeof HI;

/** A new string in the language, its slots filled. */
export const ui = wordsOf<UiKey>(HI);

/** The computer's own file manager, for "Show in Finder": Finder on the Mac, File Explorer on Windows, and on Linux,
 * whose file manager has no one name, the folder itself. */
export type Os = "mac" | "windows" | "linux";

/** The OS from what the webview reports: navigator.userAgentData.platform where there is one, else
 * navigator.platform or the user agent. */
export function osOf(platform: string): Os {
  const p = platform.toLowerCase();
  if (p.includes("win")) return "windows";
  if (p.includes("linux") || p.includes("x11")) return "linux";
  return "mac";
}

/** "Show in Finder" on the Mac, "Show in File Explorer" on Windows, "Show in folder" on Linux. */
export function revealKey(os: Os): UiKey {
  return os === "windows"
    ? "Show in File Explorer"
    : os === "linux"
      ? "Show in folder"
      : "Show in Finder";
}

/** What this webview runs on. */
export function currentOs(): Os {
  const nav = globalThis.navigator as
    | (Navigator & { userAgentData?: { platform?: string } })
    | undefined;
  if (nav === undefined) return "mac";
  return osOf(nav.userAgentData?.platform || nav.platform || nav.userAgent);
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
