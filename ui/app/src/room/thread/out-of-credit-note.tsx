// The out-of-credit note as it shows (room/connectors/out-of-credit.tsx decides when): U12's FieldNotice under the
// composer's field, "Out of case-law credits." then "Searches of Indian Kanoon stop until you add more.", the action
// "Add credits", which opens Settings at Connectors, and a quiet icon button at its end that hides it in this
// conversation. It never blocks typing or Ask: it's words under the field, nothing more. The OutOfCredit status's
// pay_url is never offered here: "Add credits" in Settings makes a fresh link. In the thread's chunk (list.tsx
// re-exports it), with its words, so the first load carries none of them.
import { FieldNotice } from "@clauseo/components/clauseo/limit-note.tsx";
import { IconCross } from "@clauseo/components/icons/icons.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { cr } from "@clauseo/components/lib/words-connectors-room.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { Tooltip } from "@clauseo/components/ui/tooltip.tsx";

/** The note; `onAdd` opens Settings at Connectors (none where the host has no settings window), `onHide` hides it in
 * this conversation (none where there is no conversation to keep it by). */
export function OutOfCreditNote({
  onAdd,
  onHide,
}: {
  onAdd: (() => void) | null;
  onHide: (() => void) | null;
}) {
  const { lang } = useDisplay();
  const hide = cr(lang, "Hide in this conversation");
  return (
    <div className="contents" data-testid="out-of-credit">
      <FieldNotice
        lead={cr(lang, "Out of case-law credits.")}
        rest={cr(lang, "Searches of Indian Kanoon stop until you add more.")}
        action={
          onAdd === null ? null : (
            <Button kind="link" size="small" data-testid="out-of-credit-add" onClick={onAdd}>
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
