// Folder rights' reason on a permission card (FR6), in the thread's chunk with the card (room/folder-reason.tsx); the
// room's notice has its own words (words-folder.ts). As in words.ts the English is the key; English only (hi: later).
import { wordsOf } from "./words-core.ts";

const HI = {
  // FR6: the reason under "Editing files" on a permission card, and behind its info icon
  "Can't set folder permissions": null, // hi: later
  "Clauseo can't change this folder's permissions, so it asks before each edit.": null, // hi: later
} as const;

export type FolderCardKey = keyof typeof HI;

/** The card's folder-rights words in `lang`. */
export const frc = wordsOf<FolderCardKey>(HI);
