// Locate's step for a folder Claude can't start in (FR24, FR25; draft words, Rohan to approve): a folder picked to
// locate a matter whose path has a backslash in a folder's name, on macOS or Linux. New matter's warning's words
// (folder-warning.tsx, folder-backslash-words.ts), with Choose another folder first, where focus starts, which opens the
// picker again, then Locate anyway, which relocates the matter there. Escape takes the pick back, as a cancelled picker
// does. The first screen's Open the folder asks the same (FR30), with Open anyway, which makes the folder a matter. Its
// own chunk, loaded by locate-warning-host.tsx when a pick is held, and by the first screen.
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { say } from "@clauseo/components/lib/words-ui.ts";
import { fr } from "@clauseo/components/lib/words-folder.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import {
  AlertDialog,
  AlertDialogClose,
  AlertDialogPopup,
} from "@clauseo/components/ui/alert-dialog.tsx";
import { Button } from "@clauseo/components/ui/button.tsx";
import { backslashAt } from "./folder-backslash.ts";
import { backslashWords } from "./folder-backslash-words.ts";
import type { HeldPick } from "./locate-held.ts";

export function LocateWarning({
  held,
  onClose,
  anyway = "Locate anyway",
  testId = "locate",
}: {
  held: HeldPick;
  onClose: () => void;
  /** The other button's words: Locate's, or the first screen's Open anyway. */
  anyway?: "Locate anyway" | "Open anyway";
  /** The step's test ids' stem: `{testId}-warning`, `-choose`, `-anyway`. */
  testId?: string;
}) {
  const { lang } = useWords();
  const { platform } = useDisplay();
  const words = backslashWords(
    lang,
    backslashAt(held.display, platform) ?? { self: true },
    platform,
  );
  return (
    <AlertDialog open onOpenChange={(open) => !open && onClose()}>
      <AlertDialogPopup
        data-testid={`${testId}-warning`}
        undoable={false}
        title={say(words.title)}
        body={say(words.body)}
        actions={
          <>
            <AlertDialogClose
              render={<Button kind="primary" data-testid={`${testId}-choose`} />}
              onClick={() => void held.again()}
            >
              <span className="deva-nudge">{fr(lang, "Choose another folder")}</span>
            </AlertDialogClose>
            <AlertDialogClose
              render={<Button kind="quiet" data-testid={`${testId}-anyway`} />}
              onClick={() => void held.anyway()}
            >
              <span className="deva-nudge">{fr(lang, anyway)}</span>
            </AlertDialogClose>
          </>
        }
      />
    </AlertDialog>
  );
}
