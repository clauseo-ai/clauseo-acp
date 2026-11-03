// The words of the settings window: its sections' names, what each says beside its name, each section's one line,
// and the words of the sections built so far (plan and usage, language, appearance, about). As in words.ts the English is the key,
// the exact string on screen, and a key with slots fills them in either language. The Hindi is a specialist's draft,
// waiting for a Hindi-speaking lawyer: the formal आप, the danda, Western digits, English names in Latin script. A
// later section brings its own strings with it. The window's title, Settings, is words-ui.ts's; Theme, Text size and
// Reduced motion are the frame's (words-frame.ts); Lite mode and its choices are words-ui.ts's.
import { useDisplay, type Lang } from "./display.ts";
import { frame, type FrameKey } from "./words-frame.ts";
import { say, ui, type UiKey } from "./words-ui.ts";
import { wordsOf, type Slots } from "./words-core.ts";

const HI = {
  // the window and its sections
  Sections: "अनुभाग",
  Language: "भाषा",
  Appearance: "दिखावट",
  About: "परिचय",
  "Plan and usage": "प्लान और उपयोग",
  // what each section says beside its name, at a glance
  "{theme}, text size {n}": "{theme}, अक्षरों का आकार {n}",
  "Version {version}": "संस्करण {version}",
  // a row the firm's policy locks
  "Managed by your organisation": "आपके संगठन द्वारा प्रबंधित",
  // each section's one line
  "The language of every word on screen. Papers, quotes and case names stay as filed.":
    "स्क्रीन के हर शब्द की भाषा। दस्तावेज़, उद्धरण और मामलों के नाम वैसे ही रहते हैं जैसे दर्ज हैं।",
  "How Clauseo looks on this computer.": "इस कंप्यूटर पर Clauseo कैसा दिखे।",
  "What each plan has left, and when it resets.": "हर प्लान में कितना बाकी है, और वह कब रीसेट होता है।",
  // plan and usage: the plan's own words are the limits' (words-limits.ts)
  "Updated at {time}": "{time} बजे अपडेट किया गया",
  // language and appearance
  "Changes at once, and everything stays where it was.": "तुरंत बदलती है, और सब कुछ वहीं रहता है जहाँ था।",
  Day: "दिन",
  Night: "रात",
  // appearance's preview: the room in small, as the choices make it
  "How it looks": "यह कैसा दिखता है",
  "A strong case, because the complaint names Mr. Sharma only as a director.":
    "मामला मज़बूत है, क्योंकि शिकायत में श्री शर्मा को केवल निदेशक के रूप में नामित किया गया है।",
} as const;

/** Every string of the settings window, in English. */
export type SettingsKey = keyof typeof HI;

/** A settings string in the language, its slots filled. */
export const st = wordsOf<SettingsKey>(HI);

/** Every string, English and Hindi, for the tests that hold the voice. */
export const settingsWords: Readonly<Record<SettingsKey, string>> = HI;

/** Each language by its own name, the same in either language: the language switch's sides, and what Language says
 * beside its name. */
export const LANGUAGE_NAMES: Readonly<Record<Lang, string>> = { en: "English", hi: "हिन्दी" };

/** Appearance's preview wall: three papers as filed, so their titles are the same in either language (the
 * design's preview, design/03-system/src/pages/settings/sections-b.tsx). */
export const PREVIEW_PAPERS: readonly {
  num: number;
  title: string;
  role?: "supports" | "against";
}[] = [
  { num: 14, title: "Form DIR-12", role: "supports" },
  { num: 31, title: "Board minutes", role: "against" },
  { num: 13, title: "Legal notice" },
];

/** The settings window's words in the page's language, bound, with the frame's and the controls' beside them: const
 * w = useSettingsWords(); w.s("Language"), w.f("Theme"), w.ui("Lite mode"); say renders with English runs marked. */
export function useSettingsWords() {
  const { lang } = useDisplay();
  return {
    lang,
    s: (key: SettingsKey, slots?: Slots) => st(lang, key, slots),
    f: (key: FrameKey, slots?: Slots) => frame(lang, key, slots),
    ui: (key: UiKey, slots?: Slots) => ui(lang, key, slots),
    say,
  };
}
