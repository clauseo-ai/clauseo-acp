// The words of a matter folder Clauseo can't fence (folder rights, FR1 to FR6, FR12, FR15): the room's notice, the
// IT text it opens; the reason on a permission card (FR6) is words-folder-card.ts's. Loaded with the notice
// (room/folder-notice.tsx), never on first paint. As in words.ts the English is the key. Every line here is new and English only (hi: later).
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-folder.hi.ts";

export type FolderKey = keyof typeof HI;

/** The folder-rights words in `lang`, their slots filled. */
export const fr = wordsOf("folder");
