// The Hindi of words-windows-line.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every
// words file's Hindi into one lazy chunk, and words-windows-line.ts imports this file's type alone. Both lines are
// provisional with Hindi later (hi: later).
import type { Hindi } from "./words-core.ts";

export const HI = {
  // UC-820: what Clauseo can't keep apart on Windows
  "On Windows, Clauseo stops Claude's and ChatGPT's file tools from opening or changing another matter's files, and refuses their commands that name another matter's folder. It can't catch everything: a program they write and run, a file location they piece together, or a shortcut they make first can still reach those files.":
    null,
  // UC-850: the Mac advice after it, Rohan's call, its own entry so it can go alone
  "For matters that must stay strictly apart, use a Mac.": null,
} satisfies Hindi<string>;
