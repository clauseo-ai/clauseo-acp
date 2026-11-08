// The Hindi of words-first-read.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every
// words file's Hindi into one lazy chunk, and words-first-read.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  // UD2-12, UD2-13: the conversation's title, and its question as the thread shows it
  "First read": "प्रारंभिक अध्ययन",
  "Clauseo's first read": "Clauseo का प्रारंभिक अध्ययन",
  // UD2-14 to UD2-17: while it reads
  "Reading {n} files.": "{n} फ़ाइलें पढ़ी जा रही हैं।",
  "Reading {n} files. Now {title}.": "{n} फ़ाइलें पढ़ी जा रही हैं। अभी {title}।",
  "Reading 1 file.": "1 फ़ाइल पढ़ी जा रही है।",
  "Reading 1 file. Now {title}.": "1 फ़ाइल पढ़ी जा रही है। अभी {title}।",
  // UD2-18, UD2-19: read
  "Read {n} files.": "{n} फ़ाइलें पढ़ी गईं।",
  "Read 1 file.": "1 फ़ाइल पढ़ी गई।",
  // UD2-20: stopped
  "Clauseo stopped. The answer so far is kept.": "Clauseo रुक गया। अब तक का उत्तर सुरक्षित है।",
} satisfies Hindi<string>;
