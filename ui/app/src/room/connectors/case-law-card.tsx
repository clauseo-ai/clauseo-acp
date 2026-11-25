// The home card's look (case-law-offer.tsx decides when it shows): NoticeCard's look with no rows, the case-law glyph
// in its tile (IconSearch, which ToolKindIcon draws for case_law), the connector's own name as the title, one sentence on what the lawyer gets, then the price and the terms as two lines ("Prepaid credits. No subscription."
// alone, with no figure, while the host lists no price), and the button that opens Clauseo Case Law in Skills and
// connectors, with "Powered by Indian Kanoon" beside it. No Indian Kanoon logo: their terms forbid it without
// written permission. At the reading column's top, as the new conversation's line is, so the composer keeps its place.
import { NoticeCard } from "@clauseo/components/clauseo/limit-note.tsx";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { IconSearch } from "@clauseo/components/icons/icons.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { rupees } from "@clauseo/components/lib/money.ts";
import { cr } from "@clauseo/components/lib/words-connectors-room.ts";
import { Button } from "@clauseo/components/ui/button.tsx";

/** The smallest pack as the host lists it: its gross price (`price_paise`, GST included), its credits, and what it
 * buys in the server's own words (store/research-questions.ts), null when it sent none. */
export type Pack = { paise: number; credits: number; questions: string | null };

export function CaseLawCard({ pack, onAdd }: { pack: Pack | null; onAdd: () => void }) {
  const { lang } = useDisplay();
  const gets = cr(
    lang,
    "Find and read Supreme Court, High Court and tribunal judgments and central Acts, and see who cites each judgment.",
  );
  // the price from the host's live price list, never a figure kept here; none while the host lists no price. What a
  // pack buys only in the server's words, as written: the app works out no count (one real question took 393 credits)
  const cost =
    pack === null
      ? null
      : pack.questions === null
        ? cr(lang, "From {price}.", { price: rupees(pack.paise) })
        : cr(lang, "From {price}, for {questions}.", {
            price: rupees(pack.paise),
            questions: pack.questions,
          });
  return (
    <div className="w-full shrink-0 px-thread-pad pb-s4" data-testid="case-law-offer">
      <div className="mx-auto max-w-measure">
        <NoticeCard
          icon={<IconSearch size={18} />}
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
