// The home card's look (case-law-offer.tsx decides when it shows): NoticeCard's look with no rows, the case-law glyph
// in its tile (ToolKindIcon's case_law, as the connector's steps draw it), the connector's own name as the title, one
// sentence on what the lawyer gets, then the price and the terms as two lines ("Prepaid credits. No subscription."
// alone, with no figure, while the host lists no price), and the button that opens Clauseo Case Law in Skills and
// connectors, with "Powered by Indian Kanoon" beside it. No Indian Kanoon logo: their terms forbid it without
// written permission. At the reading column's top, as the new conversation's line is, so the composer keeps its place.
import { NoticeCard } from "@clauseo/components/clauseo/limit-note.tsx";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { ToolKindIcon } from "@clauseo/components/clauseo/tool-kind-icons.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { rupees } from "@clauseo/components/lib/money.ts";
import { cr } from "@clauseo/components/lib/words-connectors-room.ts";
import { Button } from "@clauseo/components/ui/button.tsx";

/** The credits one page of search results costs (the library design's note, 24 Nov): what turns the smallest pack's
 * credits into "about {n} searches". The host doesn't send it before an account exists; change it here if it moves. */
const SEARCH_PAGE_CREDITS = 20;

/** The smallest pack as the host lists it: its gross price (`price_paise`, GST included) and its credits. */
export type Pack = { paise: number; credits: number };

export function CaseLawCard({ pack, onAdd }: { pack: Pack | null; onAdd: () => void }) {
  const { lang } = useDisplay();
  const gets = cr(
    lang,
    "Find and read Supreme Court, High Court and tribunal judgments and central Acts, and see who cites each judgment.",
  );
  // the price from the host's live price list, never a figure kept here; none while the host lists no price
  const cost =
    pack === null
      ? null
      : cr(lang, "From {price}, about {n} searches", {
          price: rupees(pack.paise),
          n: String(Math.max(1, Math.round(pack.credits / SEARCH_PAGE_CREDITS))),
        });
  return (
    <div className="w-full shrink-0 px-thread-pad pb-s4" data-testid="case-law-offer">
      <div className="mx-auto max-w-measure">
        <NoticeCard
          icon={<ToolKindIcon kind="case_law" size={18} />}
          label={cr(lang, "Clauseo Case Law")}
          title={cr(lang, "Clauseo Case Law")}
          sub={
            <>
              <Say>{gets}</Say>
              {cost === null ? (
                <span className="block" data-testid="case-law-offer-terms">
                  <Say>{cr(lang, "Prepaid credits. No subscription.")}</Say>
                </span>
              ) : (
                <>
                  <span className="block" data-testid="case-law-offer-price">
                    <Say>{cost}</Say>
                  </span>
                  <span className="block" data-testid="case-law-offer-terms">
                    <Say>{cr(lang, "Prepaid, including GST. No subscription.")}</Say>
                  </span>
                </>
              )}
            </>
          }
          actions={
            <>
              <Button kind="tone" size="small" data-testid="case-law-offer-add" onClick={onAdd}>
                {cr(lang, "Add Clauseo Case Law")}
              </Button>
              <span className="lim-note-line" data-testid="case-law-offer-source">
                {cr(lang, "Powered by Indian Kanoon")}
              </span>
            </>
          }
        />
      </div>
    </div>
  );
}
