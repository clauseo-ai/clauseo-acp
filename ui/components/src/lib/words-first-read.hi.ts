// The Hindi of words-first-read.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every
// words file's Hindi into one lazy chunk, and words-first-read.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  // UD2-12, UD2-13: the conversation's title, and its question as the thread shows it
  "First read": null, // hi: later
  "Clauseo's first read": null, // hi: later
  // UD2-14 to UD2-17: while it reads
  "Reading {n} files.": null, // hi: later
  "Reading {n} files. Now {title}.": null, // hi: later
  "Reading 1 file.": null, // hi: later
  "Reading 1 file. Now {title}.": null, // hi: later
  // UD2-18, UD2-19: read
  "Read {n} files.": null, // hi: later
  "Read 1 file.": null, // hi: later
  // UD2-20: stopped
  "Clauseo stopped. The answer so far is kept.": null, // hi: later
} satisfies Hindi<string>;
