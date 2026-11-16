// The first screen's step for a folder New matter would warn about (folder rights, FR7 to FR10, FR13, FR14): a folder
// picked by Open the folder on a network drive, or on a drive without file permissions. New matter's warning's words
// (folder-warning.tsx): its title, what it says, and why, with Choose another folder first, where focus starts, which
// opens the picker again, then Open anyway, which makes the folder a matter. Escape takes the pick back, as a cancelled
// picker does. The same step shape as Locate's (locate-warning.tsx, with a folder whose name has a backslash). Its own
// chunk, loaded by the first screen when a pick is held (first-folder-check.ts).
import { fr, type FolderKey } from "@clauseo/components/lib/words-folder.ts";
import { say } from "@clauseo/components/lib/words-ui.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import {
  AlertDialog,
  AlertDialogClose,
  AlertDialogPopup,
} from "@clauseo/components/ui/alert-dialog.tsx";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { DriveWarned, HeldFolder } from "./first-folder-check.ts";

/** Each warning's words, New matter's (folder-warning.tsx): its title, what it says, and why. */
const SAYS: Readonly<Record<DriveWarned, { head: FolderKey; info: FolderKey }>> = {
  network: {
    head: "On a network drive",
    info: "Clauseo can't keep Claude and ChatGPT inside a folder on a network drive.",
  },
  no_permissions: {
    head: "No file permissions",
    info: "Clauseo can't keep Claude and ChatGPT inside a folder on this drive. USB drives often don't keep permissions.",
  },
};

export function FirstFolderStep({
  held,
  onClose,
  testId = "first-folder",
}: {
  held: HeldFolder;
  onClose: () => void;
  /** The step's test ids' stem: `{testId}-warning`, `-choose`, `-anyway`. */
  testId?: string;
}) {
  const { lang } = useWords();
  const says = SAYS[held.kind];
  return (
    <AlertDialog open onOpenChange={(open) => !open && onClose()}>
      <AlertDialogPopup
        data-testid={`${testId}-warning`}
        data-kind={held.kind}
        undoable={false}
        title={say(fr(lang, says.head))}
        body={say(
          `${fr(lang, "Claude asks before every change. ChatGPT can't work here.")} ${fr(lang, says.info)}`,
        )}
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
              <span className="deva-nudge">{fr(lang, "Open anyway")}</span>
            </AlertDialogClose>
          </>
        }
      />
    </AlertDialog>
  );
}
