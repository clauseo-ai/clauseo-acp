// Settings › Tools (UC-169 to UC-189): the lawyer's own MCP servers, called tools, on this computer. As in words.ts the
// English is the key, the exact string on screen; Hindi provisional. The words that other screens already say (Edit,
// Remove, Save, Cancel, Add) are theirs, reused. {example} is a full path (the Command field) or a full address (the
// Address field), as filed, never translated.
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-mcp-tools.hi.ts";

/** Every string of Settings › Tools, in English. */
export type McpToolsKey = keyof typeof HI;

/** A Settings › Tools string in the language, its slots filled. */
export const mw = wordsOf("mcp-tools");
