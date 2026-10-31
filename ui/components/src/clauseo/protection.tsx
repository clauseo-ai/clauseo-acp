// Not protected, explained once. The first time a matter stands without protection, because the lawyer switched it
// off or the computer wouldn't set it up, the system's alert dialog says what that means in plain lines: what Clauseo
// can now reach and what is still kept. Understood, the act, is the dark primary and takes the starting focus. The
// app shows it once for a matter; the mark on the room's line stays. The open lock leads it, in concrete: nothing
// here is red.
import { AlertDialog, AlertDialogClose, AlertDialogPopup } from "../ui/alert-dialog.tsx";
import { Button } from "../ui/button.tsx";
import { IconNotProtected } from "../icons/icons.tsx";
import { useDisplay } from "../lib/display.ts";
import { useVersionWords } from "../lib/words-versions.ts";
import type { Unprotected } from "./allowed.tsx";
import { Say } from "./say.tsx";

export function NotProtectedNote({
  open,
  matter,
  why,
  onClose,
  onProtect,
}: {
  open: boolean;
  /** The matter's name as filed. */
  matter: string;
  why: Unprotected["why"];
  onClose: () => void;
  /** Switch protection back on, or try setting it up again. */
  onProtect: () => void;
}) {
  const w = useVersionWords();
  const { platform } = useDisplay();
  return (
    <AlertDialog open={open} onOpenChange={(o) => !o && onClose()}>
      <AlertDialogPopup
        undoable
        className="np-note"
        title={
          <span className="np-note-title">
            <span className="card-icon np-note-icon">
              <IconNotProtected size={18} />
            </span>
            <span>{w.v("This matter is not protected")}</span>
          </span>
        }
        body={
          <span className="np-note-body">
            <span>
              <Say>
                {why === "off"
                  ? w.v(
                      "Protection is off for {matter}. Clauseo can read and change files anywhere on this computer while it works here, without asking first.",
                      { matter },
                    )
                  : w.v(
                      "Protection couldn't be set up on this computer, so Clauseo asks before each change in this matter.",
                    )}
              </Say>
            </span>
            {why === "setup" && (
              <span>
                <Say>
                  {platform === "windows"
                    ? w.v("Windows didn't allow the protected space on this computer.")
                    : platform === "linux"
                      ? w.v("Linux didn't allow the protected space on this computer.")
                      : w.v("macOS didn't allow the protected space on this computer.")}
                </Say>
              </span>
            )}
            <span>
              <Say>
                {w.v(
                  "Every change inside the matter's folder is still kept, and Versions can undo it.",
                )}
              </Say>
            </span>
          </span>
        }
        actions={
          <>
            <AlertDialogClose render={<Button kind="quiet" />} onClick={onProtect}>
              <span className="deva-nudge">
                {why === "off" ? w.v("Switch protection on") : w.v("Try again")}
              </span>
            </AlertDialogClose>
            <AlertDialogClose render={<Button kind="primary" />}>
              <span className="deva-nudge">{w.v("Understood")}</span>
            </AlertDialogClose>
          </>
        }
      />
    </AlertDialog>
  );
}
