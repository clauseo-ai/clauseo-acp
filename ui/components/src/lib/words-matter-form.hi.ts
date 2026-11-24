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
  "Clauseo creates this folder. Add papers to it any time.":
    "Clauseo यह फ़ोल्डर बना देगा। इसमें कभी भी दस्तावेज़ जोड़ें।",
  // under the name, when Create is tried without one
  "Name the matter.": "मामले का नाम लिखें।",
  // under the folder row, when Create is tried without a folder
  "Choose a folder for the matter.": "मामले के लिए फ़ोल्डर चुनें।",
  "Create matter": "मामला बनाएँ",
  Back: "वापस",
  Cancel: "रद्द करें",
  // New matter's first question: its two answers, each with what it means for the lawyer's files (Hindi later)
  "New folder": null,
  "Clauseo makes an empty folder for this matter. Add papers to it any time.": null,
  "Existing folder": null,
  "Use a folder that already holds the papers. It stays where it is, and nothing is moved or copied.":
    null,
  // in the folder row once a folder is there: opens the computer's own folder picker for another
  "Change…": null,
  // under the folder row of an existing folder
  "The folder stays where it is. Nothing is moved or copied.": null,
  // the new folder's row where the host has no place for new matters: Choose folder… picks where it goes
  "Choose where it goes.": null,
  // UC-1250 to UC-1254 (provisional; Hindi later): New matter, after a pick macOS guards apart from Documents, the
  // reason line beside Allow, one per kind (stage E's PickedFolder.permission)
  "Clauseo needs macOS's permission to read and save papers in your Desktop folder.": null,
  "Clauseo needs macOS's permission to read and save papers in your Downloads folder.": null,
  "Clauseo needs macOS's permission to read and save papers on this external drive.": null,
  "Clauseo needs macOS's permission to read and save papers on this network drive.": null,
  "Clauseo needs macOS's permission to read and save papers in this cloud folder.": null,
  // UC-1255 (provisional; Hindi later): Allow turned down or dismissed; Create still works. {path}: where in System
  // Settings, as the desktop answers it ("System Settings › Privacy & Security › Files and Folders")
  "Without it, Clauseo can't open or save papers in this folder. You can allow it later in {path}.":
    null,
} as const;

/** Every string, English and Hindi, for the tests that hold the voice. */
export const matterFormWords: Hindi<keyof typeof HI> = HI;
