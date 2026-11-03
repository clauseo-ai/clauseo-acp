// The tool line's own words: each of Clauseo's tools by name, and where it stands in a few words. The status line
// shows the tools as glyphs alone (DESIGN.md §Icons); these words are its accessible name and its tooltip, so they
// are in the first paint, apart from words-tools.ts (the popover's and the download's). The popover's rows take the
// names and reasons from here too. English is the key; the Hindi comes later, so each entry is null (hi: later) and
// shows its English.
import { wordsOf, type Hindi } from "./words-core.ts";

const HI = {
  // hi: later: every string here ships in English, its Hindi null until the Hindi pass
  // each tool's name
  "Web search": null,
  Browser: null,
  // the tool line's name and tooltip, one sentence per tool: "Web search: on. Browser: off, needs Chrome."
  "{tool}: {state}.": null,
  on: null,
  // {reason} is one of the reasons below, its first letter in lower case
  "off, {reason}": null,
  // why a tool is off, a few words
  "Off in Settings": null,
  "Managed by firm": null,
  "Can't search on this login": null,
  "Not available yet": null,
  "Too many browsers open": null,
  "Needs Chrome": null,
} as const;

/** Every string of the tool line, in English. */
export type ToolsLineKey = keyof typeof HI;

/** A tool line string in the language, its slots filled. */
export const toolsLine = wordsOf<ToolsLineKey>(HI);

/** Every string, English and Hindi, for the tests that hold the voice. */
export const toolsLineWords: Hindi<ToolsLineKey> = HI;
