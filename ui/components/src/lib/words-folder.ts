// The words of a matter folder Clauseo can't fence (folder rights, FR1 to FR6, FR12, FR15): the room's notice, the
// IT text it opens; the reason on a permission card (FR6) is words-folder-card.ts's. Loaded with the notice
// (room/folder-notice.tsx), never on first paint. As in words.ts the English is the key. Every line here is new and English only (hi: later).
import { wordsOf } from "./words-core.ts";

const HI = {
  // FR1: the notice's title, then its tooltip and accessible name
  "Clauseo asks before each edit": null, // hi: later
  "Clauseo asks before each edit in this folder": null, // hi: later
  // FR2: behind FR1's info icon
  "You can change files here, but not this folder's permissions. Clauseo needs them to keep Claude and Codex inside the matter. Some edits won't go through.":
    null, // hi: later
  // FR3: the notice's button, which opens FR5
  "What to tell IT": null, // hi: later
  // FR5: the text to send IT, selected, with Copy; {path} is the matter's folder
  'Please give my account "Change permissions" on this folder: {path}. Clauseo, the app I use for this matter, adds one entry to the folder\'s permissions so Claude and Codex, which it runs, can write only inside it.':
    null, // hi: later
  Copy: null, // hi: later
  // FR12: a network drive's notice: its words, its tooltip and accessible name, and behind its info icon
  "On a network drive.": null, // hi: later
  "Claude asks before every change. Codex can't work here.": null, // hi: later
  "This matter's folder is on a network drive": null, // hi: later
  "Clauseo can't keep Claude and Codex inside this folder. For full protection, move the matter to a folder on this computer.":
    null, // hi: later
  // FR15: a drive without file permissions, as FR12
  "No file permissions.": null, // hi: later
  "This matter's folder is on a drive that doesn't keep file permissions": null, // hi: later
  "Clauseo can't keep Claude and Codex inside this folder. USB drives often don't keep permissions. For full protection, move the matter to a folder on this computer.":
    null, // hi: later
} as const;

export type FolderKey = keyof typeof HI;

/** The folder-rights words in `lang`, their slots filled. */
export const fr = wordsOf<FolderKey>(HI);
