// What Undo asks when a paper was edited after Clauseo's change: the alert dialog, with Keep current as the safe
// choice and first in focus. Its own chunk, loaded when Undo comes within reach.
import { useRef } from "react";
import { AlertDialog, AlertDialogClose, AlertDialogPopup } from "../ui/alert-dialog.tsx";
import { Button } from "../ui/button.tsx";
import { useThreadWords } from "../lib/words-thread.ts";

/** A path or name set apart from the words around it, so a right-to-left name can't reorder them. */
const isolate = (name: string) => `\u2068${name}\u2069`;

export function UndoAsk({
  name,
  open,
  onOpenChange,
  onUndo,
}: {
  /** The paper edited since, as shown on the wall. */
  name: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Undo anyway. Keep current does nothing but close. */
  onUndo: () => void;
}) {
  const w = useThreadWords();
  const keep = useRef<HTMLButtonElement>(null);
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogPopup
        initialFocus={keep}
        title={w.th("{name} changed after Clauseo's edit.", { name: isolate(name) })}
        body={w.ui(
          "Undo anyway puts back the version from before Clauseo's edit. The changes made since stay in Versions.",
        )}
        actions={
          <>
            <AlertDialogClose render={<Button kind="quiet" />} onClick={onUndo}>
              {w.ui("Undo anyway")}
            </AlertDialogClose>
            <AlertDialogClose render={<Button kind="tone" ref={keep} />}>
              {w.ui("Keep current")}
            </AlertDialogClose>
          </>
        }
      />
    </AlertDialog>
  );
}
