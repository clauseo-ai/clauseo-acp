// The Hindi of words-matter-form.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-matter-form.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  "A new matter": "नया मामला",
  "Matter name": "मामले का नाम",
  "Client (optional)": "मुवक्किल (वैकल्पिक)",
  Folder: "फ़ोल्डर",
  // the folder row before a folder is chosen
  "No folder chosen": "कोई फ़ोल्डर नहीं चुना गया",
  // opens the computer's own folder picker at once
  "Choose folder…": "फ़ोल्डर चुनें…",
  // {folder}: where Clauseo keeps new matters, "~/Documents/Clauseo"
  "Use a new folder in {folder}": "{folder} में नया फ़ोल्डर इस्तेमाल करें",
  // under the folder row, once the new folder is chosen
  "Clauseo makes this folder. Add papers to it any time.":
    "Clauseo यह फ़ोल्डर बनाता है। इसमें कभी भी दस्तावेज़ जोड़ें।",
  // under the name, when Create is tried without one
  "Name the matter.": "मामले का नाम लिखें।",
  // under the folder row, when Create is tried without a folder
  "Choose a folder for the matter.": "मामले के लिए फ़ोल्डर चुनें।",
  "Create matter": "मामला बनाएँ",
  Back: "वापस",
  Cancel: "रद्द करें",
} as const;

/** Every string, English and Hindi, for the tests that hold the voice. */
export const matterFormWords: Hindi<keyof typeof HI> = HI;
