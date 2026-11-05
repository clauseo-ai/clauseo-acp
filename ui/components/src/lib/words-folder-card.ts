// Folder rights' reason on a permission card (FR6), in the thread's chunk with the card (room/folder-reason.tsx); the
// room's notice has its own words (words-folder.ts). As in words.ts the English is the key; English only (hi: later).
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-folder-card.hi.ts";

export type FolderCardKey = keyof typeof HI;

/** The card's folder-rights words in `lang`. */
export const frc = wordsOf("folder-card");
