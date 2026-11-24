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
  /** Clauseo had no answer from it: Retry reads the list again, which checks again. */
  | "unreachable"
  /** It answered but isn't offered yet, or this install has no connectors: no button. */
  | "unavailable"
  /** The lawyer turned connectors off. */
  | "off"
  /** Added once, but its sign-in ended or its key was revoked. */
  | "ended"
  /** Anything else in its way: the row's word for its status. */
  | "trouble";

/** What the card shows, from the host's status for it (list_addons) and the connectors' state. The
 * firm's and the lawyer's off come first; a lawyer who holds credits is never offered them, and
 * before the connectors' state is heard the offer waits. */
export function caseLawShown(a: AddonView, facts: CaseLawFacts): CaseLawShown {
  if (facts.off === "lawyer") return "off";
  if (facts.off === "firm" || a.status === "blocked") return "trouble";
  if (a.status === "not_offered") return "unavailable";
  if (a.status === "unreachable") return "unreachable";
  if (a.status === "ready" || a.status === "needs_credit" || facts.credits !== null) return "added";
  if (a.status === "signed_out" || facts.signIn === "ended") return "ended";
  if (a.status === "signing_in" || facts.signIn === "signing_in" || !facts.heard) return "waiting";
  // the offer only when it is truly not added; "not yet checked" is needs_setup too
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

/** "1,240 credits left"; out of credit (store/case-law-credit.ts), what Settings and the composer say: "Not enough
 * credits for a search. Add credits to keep searching." with the figure left when one was read, or at 0 "No credits.
 * Add credits to search.". */
export function creditLines(lang: Lang, credits: number | null, out: boolean): string[] {
  const left = (n: number) =>
    lb(lang, "{credits} credits left", { credits: n.toLocaleString("en-IN") });
  if (credits === 0) return [cr(lang, "No credits. Add credits to search.")];
  if (out) {
    const line = cr(lang, "Not enough credits for a search. Add credits to keep searching.");
    return credits === null || credits < 0 ? [line] : [line, left(credits)];
  }
  return credits === null ? [] : [left(credits)];
}
