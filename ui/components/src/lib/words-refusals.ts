// The one line under a step's row when Claude's own checks refused it before it ran (#plain-refusal): Claude Code's
// built-in safety check (in auto mode), or a rule Clauseo set for the matter. The agent's raw refusal stays in the
// call's output, for the transcript and diagnostics; the lawyer reads one of these. As in words.ts the English is the
// key, the exact string on screen, and Hindi the value. Nothing here says I, step, session, safe, command,
// classifier or auto mode. Only the thread's chunk imports this file.
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-refusals.hi.ts";

/** Every refusal line, in English. */
export type RefusalKey = keyof typeof HI;

/** A refusal line in the language. */
export const refusal = wordsOf("refusals");
