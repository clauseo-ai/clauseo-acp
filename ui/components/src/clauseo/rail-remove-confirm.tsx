// The confirm before Remove from the list (UC-131, UC-132): nothing brings a removed matter back from the list, so the
// lawyer reads what happens first. The matter and its conversations leave the list and Recent; its folder and papers
// stay where they are, and importing the folder again brings it back. Cancel first, where focus starts (it can't be
// undone from here), then the act in the menu's own words. Confirming calls onConfirm once; Cancel, Escape or closing
// sends nothing. A matter whose folder sits inside another listed matter's (the host's `inside`) adds what removing
// it opens: that matter's conversations may then read its papers (UC-485).
import { useDisplay } from "../lib/display.ts";
import { isolate } from "../lib/words-core.ts";
import { say, ui } from "../lib/words-ui.ts";
import { AlertDialog, AlertDialogClose, AlertDialogPopup } from "../ui/alert-dialog.tsx";
import { Button } from "../ui/button.tsx";

export function RemoveConfirm({
  name,
  inside,
  open,
  onOpenChange,
  onConfirm,
}: {
  /** The matter's name as filed. */
  name: string;
  /** Optional: the name of the listed matter whose folder holds this one's. */
  inside?: string | undefined;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** forget_matter, sent only from here. */
  onConfirm: () => void;
}) {
  const { lang } = useDisplay();
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogPopup
        data-testid="remove-confirm"
        undoable={false}
        title={say(ui(lang, "Remove {name} from the list?", { name }))}
        body={say(
          [
            ui(
              lang,
              "The matter and its conversations leave the list and Recent. Its folder and papers stay where they are.",
            ),
            ...(inside === undefined
              ? []
              : [
                  ui(
                    lang,
                    "Its folder is inside {outer}'s folder, so conversations in {outer} will be able to read its papers.",
                    { outer: isolate(inside) },
                  ),
                ]),
          ].join(" "),
        )}
        actions={
          <>
            <AlertDialogClose render={<Button kind="quiet" data-testid="remove-confirm-cancel" />}>
              {ui(lang, "Cancel")}
            </AlertDialogClose>
            <AlertDialogClose
              render={<Button kind="tone" data-testid="remove-confirm-ok" />}
              onClick={onConfirm}
            >
              {ui(lang, "Remove from the list")}
            </AlertDialogClose>
          </>
        }
      />
    </AlertDialog>
  );
}
