// The words for a Clauseo that couldn't open, one line per start-up code, for the full-window line H18 brings
// (startup-failed, no actions: a second copy of Clauseo raises the first window, so store.locked has none). Each says
// why Clauseo didn't open and what is true of the lawyer's papers, which live in their own folders and are never in
// Clauseo's data. Apart from words-errors.ts, so the room's first load carries none of it until a window shows them.
// As in words.ts the English is the key, the exact string on screen. The Hindi is a specialist's draft, waiting for a
// Hindi-speaking lawyer: the formal आप, the danda, Western digits, English names in Latin script.
import type { Lang } from "./display.ts";
import { wordsOf, type Hindi } from "./words-core.ts";
import type { ErrorCode } from "./words-errors.ts";

/** The codes a failed start brings to the window (H18). */
export type StartupCode = Extract<
  ErrorCode,
  "store.newer_version" | "store.corrupt" | "store.disk_full" | "store.io"
>;

const HI = {
  "Clauseo couldn't open. A newer version of Clauseo saved its data, so this version can't read it. Your papers stay in their folders.":
    "Clauseo नहीं खुल सका। इसका डेटा Clauseo के नए संस्करण ने सहेजा है, इसलिए यह संस्करण उसे पढ़ नहीं सकता। आपके दस्तावेज़ अपने फ़ोल्डरों में ही हैं।",
  "Clauseo couldn't open. Its saved data doesn't read back. Your papers stay in their folders.":
    "Clauseo नहीं खुल सका। इसका सहेजा गया डेटा वापस नहीं पढ़ा जा रहा। आपके दस्तावेज़ अपने फ़ोल्डरों में ही हैं।",
  "Clauseo couldn't open. The computer's disk is full. Free some space, then open Clauseo again.":
    "Clauseo नहीं खुल सका। कंप्यूटर की डिस्क भर गई है। कुछ जगह खाली करें, फिर Clauseo दोबारा खोलें।",
  "Clauseo couldn't open. It couldn't read or write its data. Open Clauseo again.":
    "Clauseo नहीं खुल सका। यह अपना डेटा पढ़ या लिख नहीं सका। Clauseo दोबारा खोलें।",
} as const;

/** Every start-up line, in English. */
export type StartupKey = keyof typeof HI;

/** One line per start-up code: a code with no line fails typecheck. */
export const STARTUP_WORDS: Record<StartupCode, StartupKey> = {
  "store.newer_version":
    "Clauseo couldn't open. A newer version of Clauseo saved its data, so this version can't read it. Your papers stay in their folders.",
  "store.corrupt":
    "Clauseo couldn't open. Its saved data doesn't read back. Your papers stay in their folders.",
  "store.disk_full":
    "Clauseo couldn't open. The computer's disk is full. Free some space, then open Clauseo again.",
  "store.io": "Clauseo couldn't open. It couldn't read or write its data. Open Clauseo again.",
};

/** A start-up line in the language. */
export const startup = wordsOf<StartupKey>(HI);

/** A start-up code's line in the language. */
export const startupWords = (lang: Lang, code: StartupCode): string =>
  startup(lang, STARTUP_WORDS[code]);

/** Every key, for the banned-words screen in the tests. */
export const startupWordList: Hindi<StartupKey> = HI;
