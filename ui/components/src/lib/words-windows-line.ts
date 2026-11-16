// The Windows line's words (UC-820, then UC-850): what Clauseo can't keep apart on Windows, the launch build's command
// scan as the host has it, then the Mac advice. Shown at the top of every conversation on Windows once a second matter
// exists, until "Got it" (the room's notice), readable for good in Settings › Privacy on Windows, and in Settings › Safe
// workspace while it's on, once that returns. A words file of its own, so Privacy's line pulls no protection words
// into the settings window's shared chunks. Its Mac counterpart (UC-821) comes with the host's Mac fence.
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-windows-line.hi.ts";

export type WindowsLineKey = keyof typeof HI;

/** The Windows line's words in `lang`. */
export const wl = wordsOf("windows-line");

/** What Clauseo can't keep apart on Windows (UC-820). */
export const WINDOWS_LINE: WindowsLineKey =
  "On Windows, Clauseo stops Claude's and ChatGPT's file tools from opening or changing another matter's files, and refuses their commands that name another matter's folder. It can't catch everything: a program they write and run, a file location they piece together, or a shortcut they make first can still reach those files.";

/** The advice after WINDOWS_LINE, in each of its places (UC-850): a product statement only Rohan can make, its own
 * sentence and entry so it can be removed alone. */
export const WINDOWS_STRICT: WindowsLineKey =
  "For matters that must stay strictly apart, use a Mac.";
