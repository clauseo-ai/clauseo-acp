// UC: the "Unknown publisher" confirm, before a protection command that brings Windows'
// administrator prompt (Ruled, 1): before a set-up from the app (after an in-app Remove, or a
// re-run), before Add room (a full pool's top-up) and before removing the Safe workspace. Design
// B's installer does the first set-up. Windows' prompt takes the screen over and may name the
// publisher as unknown, so the lawyer reads that it is expected first. Cancel first, where focus
// starts (neither act can be undone from here), then the act in the button's own words. Shared by
// the room's note and the firm's notice, and Settings. Where the firm requires the Safe workspace,
// the remove confirm says Clauseo won't answer afterwards, never that it works without it; the
// button stays, since IT may remove it to repair it. Confirming calls onConfirm once; Cancel,
// Escape or closing sends nothing.
import { useDisplay } from "../lib/display.ts";
import { pw } from "../lib/words-protection.ts";
import { say, ui } from "../lib/words-ui.ts";
import { AlertDialog, AlertDialogClose, AlertDialogPopup } from "../ui/alert-dialog.tsx";
import { Button } from "../ui/button.tsx";

export type ProtectionConfirmAct = "setup" | "add" | "remove";

export function ProtectionConfirm({
  act,
  open,
  required = false,
  onOpenChange,
  onConfirm,
}: {
  act: ProtectionConfirmAct;
  /** Optional: the firm requires the Safe workspace (ProtectionView.required), for "remove"'s after line. */
  required?: boolean;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The command, sent only from here. */
  onConfirm: () => void;
}) {
  const { lang } = useDisplay();
  const label =
    act === "setup"
      ? pw(lang, "Set up the Safe workspace")
      : act === "add"
        ? pw(lang, "Add room")
        : pw(lang, "Remove set-up");
  const title =
    act === "setup"
      ? pw(lang, "Set up the Safe workspace?")
      : act === "add"
        ? pw(lang, "Add room for 4 more?")
        : pw(lang, "Remove the Safe workspace from this computer?");
  const ask =
    act === "setup"
      ? pw(
          lang,
          "Windows will ask to let Clauseo set up the Safe workspace. It will say the publisher is unknown. That's expected for Clauseo. Choose Yes. If Windows asks for an administrator's password, enter the password of an administrator account on this computer.",
        )
      : pw(
          lang,
          "Windows will ask to let Clauseo add room to the Safe workspace. It will say the publisher is unknown. That's expected for Clauseo. Choose Yes. If Windows asks for an administrator's password, enter the password of an administrator account on this computer.",
        );
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogPopup
        data-testid="protection-confirm"
        data-act={act}
        undoable={false}
        title={say(title)}
        body={
          act !== "remove" ? (
            say(ask)
          ) : (
            <span className="np-note-body">
              <span>
                {say(
                  pw(
                    lang,
                    "Windows will ask to let Clauseo remove the Safe workspace. It will say the publisher is unknown. That's expected for Clauseo. Choose Yes. If Windows asks for an administrator's password, enter the password of an administrator account on this computer.",
                  ),
                )}
              </span>
              <span>
                {say(
                  required
                    ? pw(
                        lang,
                        "Your firm requires the Safe workspace, so afterwards Clauseo won't answer on this computer until it's set up again.",
                      )
                    : pw(
                        lang,
                        "Afterwards, Clauseo works on this computer without the Safe workspace until it's set up again.",
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
