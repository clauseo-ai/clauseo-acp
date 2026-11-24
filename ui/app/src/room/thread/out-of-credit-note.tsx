// The out-of-credit note as it shows (room/connectors/out-of-credit.tsx decides when): U12's FieldNotice under the
// composer's field. Its first line names the connector, "Clauseo Case Law", with the figure left when one was read
// ("Clauseo Case Law · 7 credits left"); its second says what to do, the same words Settings and the library use:
// "Not enough credits for a search. Add credits to keep searching.", or at 0 "No credits. Add credits to search.".
// Then "Add credits", which opens the host's pay link or else Settings at Connectors, and a quiet icon button at its
// end that hides it in this conversation. It never blocks typing or Ask: it's words under the field, nothing more.
// Once credits arrive after the refusal, CreditsAddedNote takes its place until the next message is sent. In
// the thread's chunk (list.tsx re-exports it), with its words, so the first load carries none of them.
import { FieldNotice } from "@clauseo/components/clauseo/limit-note.tsx";
import { IconCross } from "@clauseo/components/icons/icons.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { count } from "@clauseo/components/lib/money.ts";
import { cr } from "@clauseo/components/lib/words-connectors-room.ts";
import { cw } from "@clauseo/components/lib/words-connectors.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { Tooltip } from "@clauseo/components/ui/tooltip.tsx";

/** The note; `credits` the figure left (null when none was read), `onAdd` opens the pay link or Settings at Connectors
 * (none where there is neither), `onHide` hides it in this conversation (none where there is no conversation). */
export function OutOfCreditNote({
  credits,
  onAdd,
  onHide,
}: {
  credits: number | null;
  onAdd: (() => void) | null;
  onHide: (() => void) | null;
}) {
  const { lang } = useDisplay();
  const hide = cr(lang, "Hide in this conversation");
  const name = cr(lang, "Clauseo Case Law");
  return (
    <div className="contents" data-testid="out-of-credit">
      <FieldNotice
        lead={
          credits === null || credits === 0
            ? name
            : `${name} · ${cw(lang, "{credits} credits left", { credits: count(credits) })}`
        }
        rest={
          credits === 0
            ? cr(lang, "No credits. Add credits to search.")
            : cr(lang, "Not enough credits for a search. Add credits to keep searching.")
        }
        action={
          onAdd === null ? null : (
            // inline at the end of the second sentence, as words: no padding of its own, so no stray indent
            <Button
              kind="link"
              size="small"
              className="min-h-0 px-0 py-0 text-primary align-baseline underline underline-offset-2"
              data-testid="out-of-credit-add"
              onClick={onAdd}
            >
              {cr(lang, "Add credits")}
            </Button>
          )
        }
        end={
          onHide === null ? null : (
            <Tooltip content={hide}>
              <Button
                kind="quiet"
                size="small"
                shape="icon"
                aria-label={hide}
                data-testid="out-of-credit-hide"
                onClick={onHide}
              >
                <IconCross size={14} />
              </Button>
            </Tooltip>
          )
        }
      />
    </div>
  );
}

/** Once credits arrive after this conversation's turn was refused for them: one line under the field until the next
 * message is sent. Nothing resends the question; the lawyer asks again. */
export function CreditsAddedNote() {
  const { lang } = useDisplay();
  return (
    <div className="contents" data-testid="credits-added">
      <FieldNotice lead={cr(lang, "Credits added. Ask again to search case law.")} />
    </div>
  );
}
