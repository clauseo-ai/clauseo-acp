// The tool line's own words: each of Clauseo's tools by name, On or Off, and where it stands in a few
// words. The status line shows each tool's glyph, its name and On or Off (issue 4 of 24 Nov: glyphs
// alone didn't say what they were); these words are its accessible name and its tooltip too, so they
// are in the first paint, apart from words-tools.ts (the popover's). The popover's rows take the
// names, On or Off and the reasons from here too. English is the key; its Hindi is in
// words-tools-line.hi.ts.
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-tools-line.hi.ts";

/** Every string of the tool line, in English. */
export type ToolsLineKey = keyof typeof HI;

/** A tool line string in the language, its slots filled. */
export const toolsLine = wordsOf("tools-line");
