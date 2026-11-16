// The words of the scan of a folder picked in New matter (UD2): its counts, the file it has reached, and the notices for
// a folder a cloud service syncs or keeps online. Loaded with New matter's chunk (room/scan/), never on first paint, so
// the rail's New matter loads none of the first run's words. As in words.ts the English is the key. Every line is
// English only for now (hi: later).
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-scan.hi.ts";

export type ScanKey = keyof typeof HI;

/** The scan's words in `lang`, their slots filled. */
export const sc = wordsOf("scan");
