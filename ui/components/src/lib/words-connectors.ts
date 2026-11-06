// The Connectors section's words (U15), in the settings window: Indian Kanoon's block by where it stands, the packs,
// the payment page, the API key, the lawyer's own switch and what each search sends. As in words.ts the English is
// the key, the exact string on screen; English only for now (hi: later). Money and credits fill their slots from
// money.ts, so every figure is the rupee sign, Indian grouping and Western digits in either language. The room's
// hint and banner bring their own words later (words-connectors-room.ts).
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-connectors.hi.ts";

/** Every string of the Connectors section, in English. */
export type ConnectorsKey = keyof typeof HI;

/** A Connectors string in the language, its slots filled. */
export const cw = wordsOf("connectors");
