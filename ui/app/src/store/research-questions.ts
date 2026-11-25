// What Clauseo Case Law's credits buy, in the server's own words ("about 5 research questions", "less than one research
// question"): a pack's, and the balance's. Shown as written, nothing worked out here; absent (or empty) means no count.
// The host keeps the text only when it is one plain line of at most 80 characters (#1330); empty still reads as none.
import type { Balance, PackView } from "../transport/protocol.ts";

const shown = (text: string | undefined): string | null =>
  text === undefined || text.trim() === "" ? null : text;

/** A pack's words, or null. */
export const packQuestions = (pack: PackView | null | undefined): string | null =>
  shown(pack?.research_questions_text);

/** The balance's words, or null. */
export const balanceQuestions = (balance: Balance | null | undefined): string | null =>
  shown(balance?.research_questions_text);
