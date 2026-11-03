// The tool line's own words on the status line: what Clauseo's tools can do in the open conversation, said in one
// phrase. Apart from words-tools.ts (the popover's and the download's), since the status line is in the first paint
// and the popover is not. English is the key; the Hindi comes later, so each entry is null
// (hi: later) and shows its English.
import { wordsOf, type Hindi } from "./words-core.ts";

const HI = {
  // hi: later: every string here ships in English, its Hindi null until the Hindi pass
  "Web search and browser on": null,
  "Web search on": null,
  "Web pages and browser on": null,
  "Web pages only": null,
  "Browser on, no web search": null,
  "Web search off": null,
  "Browser on": null,
  "Browser off": null,
} as const;

/** Every phrase the tool line can say, in English. */
export type ToolsLineKey = keyof typeof HI;

/** A tool line phrase in the language. */
export const toolsLine = wordsOf<ToolsLineKey>(HI);

/** Every phrase, English and Hindi, for the tests that hold the voice. */
export const toolsLineWords: Hindi<ToolsLineKey> = HI;
