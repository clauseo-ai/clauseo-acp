// The words of the new matter form, in English and Hindi. The form is a part of its own, opened by
// the first launch and by the rail's New matter, so its words live apart from either screen.
import { useDisplay } from "./display.ts";
import { wordsOf } from "./words-core.ts";

const HI = {
  "A new matter": "नया मामला",
  "Name, as it stands on the cause list": "नाम, जैसा वाद सूची में दर्ज है",
  "Client, if any": "मुवक्किल, यदि कोई हो",
  "Next date, if fixed": "अगली तारीख, यदि तय हो",
  "30 October 2026": "30 अक्टूबर 2026",
  "Folder:": "फ़ोल्डर:",
  Change: "बदलें",
  "Clauseo makes this folder. Add papers to it any time.":
    "Clauseo यह फ़ोल्डर बनाता है। इसमें कभी भी दस्तावेज़ जोड़ें।",
  "Create the matter": "मामला बनाएँ",
  Back: "वापस",
  Cancel: "रद्द करें",
} as const;

export type MatterFormKey = keyof typeof HI;
/** Every string, English and Hindi, for the tests that hold the voice. */
export const matterFormWords: Readonly<Record<MatterFormKey, string>> = HI;

export const mf = wordsOf<MatterFormKey>(HI);

/** The form's words in the page's language: const w = useMatterFormWords(); w("Create the matter"). */
export function useMatterFormWords() {
  const { lang } = useDisplay();
  return (key: MatterFormKey) => mf(lang, key);
}
