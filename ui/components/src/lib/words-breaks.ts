// The words of item 16, when something breaks: an answer Claude stopped giving (the design's limits/stopped lines, from
// its words-limits.ts) and a stream that failed. Apart from words-limits.ts and words-errors.ts, which the room's first
// paint carries, since these show only after a break: the room loads them with the break's lines (room/break-lines.ts)
// and the thread's chunk. As in words.ts the English is the key, the exact string on screen, and {time} takes clock(),
// so Hindi puts the part of the day first. The design's three lines keep the design's Hindi; the stream-failed line is
// a specialist's draft, waiting for a Hindi-speaking lawyer: the formal आप, the danda, Western digits, English names in
// Latin script.
import type { Lang } from "./display.ts";
import { wordsOf } from "./words-core.ts";
import { errorWords, type AgentName, type ErrorCode } from "./words-errors.ts";
import type { HI } from "./words-breaks.hi.ts";

/** Every string of item 16, in English. */
export type BreakKey = keyof typeof HI;

/** A string of item 16 in the language, its slots filled. */
export const brk = wordsOf("breaks");

/** For a failed stream that offers no Try again: the codes whose own sentence says to try again, said without it. */
const NO_TRY: Partial<Record<ErrorCode, BreakKey>> = {
  "store.disk_full": "The computer's disk is full. Free some space.",
};

/** The line for a stream that failed: that the thread stopped coming in, then its code's sentence; without Try again
 * (`retryable` false), a sentence that doesn't tell the lawyer to try again. `agent` is the thread's, as errorWords takes it. */
export function streamFailedWords(
  lang: Lang,
  code: string,
  retryable: boolean,
  agent?: AgentName,
): string {
  const quiet = retryable || !Object.hasOwn(NO_TRY, code) ? undefined : NO_TRY[code as ErrorCode];
  const said = quiet === undefined ? errorWords(lang, code, agent) : brk(lang, quiet);
  return `${brk(lang, "This conversation stopped updating in this window.")} ${said}`;
}
