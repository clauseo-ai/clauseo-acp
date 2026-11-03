// The tool line's own words on the status line: what Clauseo's tools can do in the open conversation, said in one
// phrase. Apart from words-tools.ts (the popover's and the download's), since the status line is in the first paint
// and the popover is not. English is the key; the Hindi waits for a translator, so each value is the English for now.
import { wordsOf } from "./words-core.ts";

const HI = {
  // hi: later, every value in this block
  "Web search and browser on": "Web search and browser on",
  "Web search on": "Web search on",
  "Web pages and browser on": "Web pages and browser on",
  "Web pages only": "Web pages only",
  "Browser on, no web search": "Browser on, no web search",
  "Web search off": "Web search off",
  "Browser on": "Browser on",
  "Browser off": "Browser off",
} as const;

/** Every phrase the tool line can say, in English. */
export type ToolsLineKey = keyof typeof HI;

/** A tool line phrase in the language. */
export const toolsLine = wordsOf<ToolsLineKey>(HI);

/** Every phrase, English and Hindi, for the tests that hold the voice. */
export const toolsLineWords: Readonly<Record<ToolsLineKey, string>> = HI;
