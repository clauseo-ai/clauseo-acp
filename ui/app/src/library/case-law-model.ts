// Clauseo Case Law's figures from the connectors' state: the price line from the smallest pack
// listed, and the credits left from the last account read.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { rupees } from "@clauseo/components/lib/money.ts";
import { cr } from "@clauseo/components/lib/words-connectors-room.ts";
import { lb } from "@clauseo/components/lib/words-library.ts";
import type { ConnectorView } from "../transport/protocol.ts";

/** The credits one page of search results costs (the design note, 24 Nov), as on the home card. */
const SEARCH_PAGE_CREDITS = 20;

const searches = (credits: number) => Math.floor(credits / SEARCH_PAGE_CREDITS);

/** The price line, "From ₹236, about 100 searches", from the smallest pack listed; null
 * while none is. */
export function priceLine(lang: Lang, view: ConnectorView | undefined): string | null {
  const pack = [...(view?.packs ?? [])].sort((a, b) => a.paise - b.paise)[0];
  if (pack === undefined) return null;
  return cr(lang, "From {price}, about {n} searches", {
    price: rupees(pack.paise),
    n: String(Math.max(1, Math.round(pack.credits / SEARCH_PAGE_CREDITS))),
  });
}

/** The credits left, from the last account read; null before one. */
export function creditsOf(view: ConnectorView | undefined): number | null {
  const s = view?.status;
  if (typeof s !== "object") return null;
  if ("out_of_credit" in s) return 0;
  if ("signed_in" in s) return s.signed_in.balance?.credits ?? null;
  return null;
}

/** "1,240 credits left", "About 62 searches", or "No credits left". */
export function creditLines(lang: Lang, credits: number): string[] {
  if (credits <= 0) return [lb(lang, "No credits left")];
  return [
    lb(lang, "{credits} credits left", { credits: credits.toLocaleString("en-IN") }),
    lb(lang, "About {n} searches", { n: String(searches(credits)) }),
  ];
}
