// R18's cards' own words: a folder, a file or a site the Safe workspace refused, as a card in the thread that asks the
// computer's own dialog (the card grants nothing), its states once asked, the line for what can't be allowed, the
// network's sign-in, and the answer paused after too many refusals. As in words.ts the English is the key, the exact
// string on screen, and a key with slots fills them in either language; the path or host is never in a sentence: it
// stands apart, in <bdi>. Provisional, waiting for Rohan's batch review and a Hindi-speaking lawyer. Nothing here says I or मैं,
// agent, session, step, token or terminal.
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-denials.hi.ts";

/** Every string of the denial cards, in English. */
export type DenialKey = keyof typeof HI;

/** A string of the denial cards in the language, its slots filled. */
export const dn = wordsOf("denials");
