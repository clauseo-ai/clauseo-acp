// Clauseo Case Law's state as its card and detail show it, from its list item and its facts
// (addons-source.ts), and the figures they say: the price line from the smallest pack listed, and the
// credits left. The app never works out how many searches credits buy: one question can take
// hundreds of credits. A lawyer who holds credits never sees the offer: before the connectors' state has
// arrived the card waits, and connectors turned off say so.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { rupees } from "@clauseo/components/lib/money.ts";
import { cr } from "@clauseo/components/lib/words-connectors-room.ts";
import { lb } from "@clauseo/components/lib/words-library.ts";
import type { AddonView } from "./addons.ts";
import type { CaseLawFacts } from "./addons-source.ts";

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
  /** Added once, but its sign-in ended or its key was revoked. */
  | "ended"
  /** Anything else in its way: the row's word for its status. */
  | "trouble";

export function caseLawShown(a: AddonView, facts: CaseLawFacts): CaseLawShown {
  if (facts.off === "lawyer") return "off";
  if (facts.off === "firm" || a.status === "blocked") return "trouble";
  if (a.status === "unreachable" || facts.signIn === "error") return "unreachable";
  if (a.status === "ready" || facts.credits !== null) return "added";
  if (facts.signIn === "ended") return "ended";
  if (facts.signIn === "signing_in" || !facts.heard) return "waiting";
  // the offer only when it is truly not added
  if (a.status === "needs_setup") return "offer";
  return "trouble";
}

/** The price and terms lines: "From ₹236." over "Prepaid, including GST. No subscription.", or, while
 * no pack is listed, "Prepaid credits. No subscription." alone. */
export function priceLines(lang: Lang, pack: CaseLawFacts["pack"]): string[] {
  if (pack === null) return [cr(lang, "Prepaid credits. No subscription.")];
  return [
    cr(lang, "From {price}.", { price: rupees(pack.paise) }),
    cr(lang, "Prepaid, including GST. No subscription."),
  ];
}

/** "1,240 credits left", or "No credits left". */
export function creditLine(lang: Lang, credits: number): string {
  return credits <= 0
    ? lb(lang, "No credits left")
    : lb(lang, "{credits} credits left", { credits: credits.toLocaleString("en-IN") });
}
