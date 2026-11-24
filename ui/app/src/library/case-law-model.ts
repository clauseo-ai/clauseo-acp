// Clauseo Case Law's state as its card and detail show it, from its list item and its facts
// (addons-source.ts), and the figures they say: the price line from the smallest pack listed, and the
// credits left. A lawyer who holds credits never sees the offer: before the connectors' state has
// arrived the card waits, and connectors turned off say so.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { rupees } from "@clauseo/components/lib/money.ts";
import { cr } from "@clauseo/components/lib/words-connectors-room.ts";
import { lb } from "@clauseo/components/lib/words-library.ts";
import type { AddonView } from "./addons.ts";
import type { CaseLawFacts } from "./addons-source.ts";

/** The credits one page of search results costs (the design note, 24 Nov), as on the home card. */
const SEARCH_PAGE_CREDITS = 20;

export type CaseLawShown =
  /** The connectors' state hasn't arrived: a card with nothing to press yet. */
  | "waiting"
  /** Not added: the offer. */
  | "offer"
  /** Added: its credits. */
  | "added"
  /** The host lists it nowhere, or its check failed. */
  | "unreachable"
  /** The lawyer turned connectors off. */
  | "off"
  /** Anything else in its way: the row's word for its status. */
  | "trouble";

export function caseLawShown(a: AddonView, facts: CaseLawFacts): CaseLawShown {
  if (facts.off === "lawyer") return "off";
  if (facts.off === "firm" || a.status === "blocked") return "trouble";
  if (a.status === "unreachable") return "unreachable";
  if (a.status === "ready" || facts.credits !== null) return "added";
  if (a.status === "needs_setup") return facts.heard ? "offer" : "waiting";
  return "trouble";
}

/** The price line, "From ₹236, about 100 searches", from the smallest pack listed; null while none
 * is. */
export function priceLine(lang: Lang, pack: CaseLawFacts["pack"]): string | null {
  if (pack === null) return null;
  return cr(lang, "From {price}, about {n} searches", {
    price: rupees(pack.paise),
    n: String(Math.max(1, Math.round(pack.credits / SEARCH_PAGE_CREDITS))),
  });
}

/** "1,240 credits left", "About 62 searches", or "No credits left". */
export function creditLines(lang: Lang, credits: number): string[] {
  if (credits <= 0) return [lb(lang, "No credits left")];
  return [
    lb(lang, "{credits} credits left", { credits: credits.toLocaleString("en-IN") }),
    lb(lang, "About {n} searches", { n: String(Math.floor(credits / SEARCH_PAGE_CREDITS)) }),
  ];
}
