// UC: the "Unknown publisher" confirm, before a protection command that brings Windows' administrator prompt (Ruled,
// 1): always before removing protection's set-up, and before the set-up when it asks an administrator. Windows' prompt
// takes the screen over and may name the publisher as unknown, so the lawyer reads that it is expected first. Cancel
// first, where focus starts (neither act can be undone from here), then the act in the button's own words. Shared by
// the room's note and the firm's notice, and Settings. Confirming calls onConfirm once; Cancel, Escape or closing
// sends nothing.
import { useDisplay } from "../lib/display.ts";
import { pw } from "../lib/words-protection.ts";
import { say, ui } from "../lib/words-ui.ts";
import { AlertDialog, AlertDialogClose, AlertDialogPopup } from "../ui/alert-dialog.tsx";
import { Button } from "../ui/button.tsx";

export type ProtectionConfirmAct = "setup" | "remove";

export function ProtectionConfirm({
  act,
  open,
  onOpenChange,
  onConfirm,
}: {
  act: ProtectionConfirmAct;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The command, sent only from here. */
  onConfirm: () => void;
}) {
  const { lang } = useDisplay();
  const setup = act === "setup";
  const label = setup ? pw(lang, "Set up protection") : pw(lang, "Remove set-up");
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogPopup
        data-testid="protection-confirm"
        data-act={act}
        undoable={false}
        title={say(
          setup ? pw(lang, "Set up protection?") : pw(lang, "Remove protection's set-up?"),
        )}
        body={
          setup ? (
            say(
              pw(
                lang,
                "Windows will ask if you want to allow changes, and it may show the publisher as unknown. That's expected. Choose Yes to set up protection.",
              ),
            )
          ) : (
            <span className="np-note-body">
              <span>
                {say(
                  pw(
                    lang,
                    "Windows will ask if you want to allow changes, and it may show the publisher as unknown. That's expected. Choose Yes to remove protection's set-up.",
                  ),
                )}
              </span>
              <span>
                {say(
                  pw(
                    lang,
                    "Afterwards, Clauseo works on this computer without protection until you set it up again.",
                  ),
                )}
              </span>
            </span>
          )
        }
        actions={
          <>
            <AlertDialogClose
              render={<Button kind="quiet" data-testid="protection-confirm-cancel" />}
            >
              {ui(lang, "Cancel")}
            </AlertDialogClose>
            <AlertDialogClose
              render={<Button kind="tone" data-testid="protection-confirm-ok" />}
              onClick={onConfirm}
            >
              {say(label)}
            </AlertDialogClose>
          </>
        }
      />
    </AlertDialog>
  );
}
