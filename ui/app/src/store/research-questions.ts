// What Clauseo Case Law's credits buy, in the server's own words ("about 5 research questions", "less than one research
// question"): a pack's, and the balance's. Shown as written, nothing worked out here; absent (or empty) means no count.
// The host bounds the text to one short plain line (#1330).
import type { Balance, PackView } from "../transport/protocol.ts";

// TODO(#1330): PackView and Balance gain `research_questions_text?: string` with #1330's bindings. Then delete this
// type and the two `as` widenings below; nothing else changes.
type Words = { research_questions_text?: string };

const shown = (text: string | undefined): string | null =>
  text === undefined || text.trim() === "" ? null : text;

/** A pack's words, or null. */
export const packQuestions = (pack: PackView | null | undefined): string | null =>
  shown((pack as (PackView & Words) | null | undefined)?.research_questions_text);

/** The balance's words, or null. */
export const balanceQuestions = (balance: Balance | null | undefined): string | null =>
  shown((balance as (Balance & Words) | null | undefined)?.research_questions_text);
