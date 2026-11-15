// Locate's step for a folder Claude can't start in (FR24, FR25; draft words, Rohan to approve): a folder picked to
// locate a matter whose name has a backslash, on macOS or Linux. New matter's warning's words (folder-warning.tsx),
// with Choose another folder first, where focus starts, which opens the picker again, then Locate anyway, which
// relocates the matter there. Escape takes the pick back, as a cancelled picker does. Its own chunk, loaded by
// locate-warning-host.tsx when a pick is held.
import { say } from "@clauseo/components/lib/words-ui.ts";
import { fr } from "@clauseo/components/lib/words-folder.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import {
  AlertDialog,
  AlertDialogClose,
  AlertDialogPopup,
} from "@clauseo/components/ui/alert-dialog.tsx";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { HeldLocate } from "./locate-held.ts";

export function LocateWarning({ held, onClose }: { held: HeldLocate; onClose: () => void }) {
  const { lang } = useWords();
  return (
    <AlertDialog open onOpenChange={(open) => !open && onClose()}>
      <AlertDialogPopup
        data-testid="locate-warning"
        undoable={false}
        title={say(fr(lang, "This folder's name has a backslash (\\)."))}
        body={say(
          fr(
            lang,
            "Claude can't work in a folder like this; ChatGPT can. Rename it in Finder first, or choose another folder.",
          ),
        )}
        actions={
          <>
            <AlertDialogClose
              render={<Button kind="primary" data-testid="locate-choose" />}
              onClick={() => void held.again()}
            >
              <span className="deva-nudge">{fr(lang, "Choose another folder")}</span>
            </AlertDialogClose>
            <AlertDialogClose
              render={<Button kind="quiet" data-testid="locate-anyway" />}
              onClick={() => void held.anyway()}
            >
              <span className="deva-nudge">{fr(lang, "Locate anyway")}</span>
            </AlertDialogClose>
          </>
        }
      />
    </AlertDialog>
  );
}
