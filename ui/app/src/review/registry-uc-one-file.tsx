// UC's entries for restoring one document from Versions (UC-316 to UC-325, and UC-428): Restore this document beside the
// documents that differ from now, the restore dialog it opens for that document alone, and the line once it is done,
// in the room with Versions open over registry-u8.tsx's host, after the presses that bring each state. The English
// is provisional until Rohan's review, its Hindi too.
import type { FileChange } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { state, type Matter } from "./registry-u8.tsx";
import { versionWords } from "./words.ts";

const NOTICE = "Notice to the bank.docx";
const changed = (change: FileChange["change"]): FileChange => ({
  path: NOTICE,
  change,
  size_bytes: 1200,
  may_include_lawyer_edits: false,
});

/** Versions open, at the turn's version three days ago or the lawyer's edits today, compared with now. */
const OPEN = ".room-pair button";
const EARLIER = '[data-testid="version"][data-checkpoint="cp2"]';
const TODAY = '[data-testid="version"][data-checkpoint="cp3"]';
const COMPARE = ".ver-compare";
const RESTORE_FILE = '[data-testid="version-restore-file"]';
const CONFIRM = '[data-testid="dialog-confirm"]';
const UNDO_DONE = ".ver-restored-undo";
const ASK_EARLIER = [OPEN, EARLIER, COMPARE, RESTORE_FILE] as const;
const ASK_TODAY = [OPEN, TODAY, COMPARE, RESTORE_FILE] as const;

/** The notice differs from now, and restoring it replaces it. */
const MODIFIED: Matter = { compare: [changed("modified")] };
/** The notice was removed since, and restoring it brings it back. */
const REMOVED: Matter = { compare: [changed("deleted")], changes: [changed("added")] };

export const UC_ONE_FILE: Entry[] = [
  state("UC-316", versionWords("Restore this document"), MODIFIED, [OPEN, EARLIER, COMPARE]),
  state(
    "UC-317",
    versionWords("Restore '{paper}' to how it was at {time} today?"),
    MODIFIED,
    ASK_TODAY,
  ),
  state(
    "UC-318",
    versionWords("Restore '{paper}' to how it was at {time} on {date}?"),
    MODIFIED,
    ASK_EARLIER,
  ),
  state(
    "UC-319",
    versionWords(
      "This replaces '{paper}' as it is now. The copy you have now stays in Versions, so you can undo this.",
    ),
    MODIFIED,
    ASK_EARLIER,
  ),
  state(
    "UC-320",
    versionWords("'{paper}' comes back as it was then. You can undo this."),
    REMOVED,
    ASK_EARLIER,
  ),
  // the notice can't be restored (a name excluded now, say): the line that it stays follows
  state(
    "UC-321",
    versionWords("Nothing will change."),
    { ...MODIFIED, changes: [], kept: [NOTICE] },
    ASK_EARLIER,
  ),
  state(
    "UC-322",
    versionWords("Nothing will change. '{paper}' is already the same as it was then."),
    { ...MODIFIED, changes: [] },
    ASK_EARLIER,
  ),
  state("UC-323", versionWords("Restored {paper} to how it was at {time} today."), MODIFIED, [
    ...ASK_TODAY,
    CONFIRM,
  ]),
  state("UC-324", versionWords("Restored {paper} to how it was at {time} on {date}."), MODIFIED, [
    ...ASK_EARLIER,
    CONFIRM,
  ]),
  // Undo on the done line, through the undo's own dialog
  state(
    "UC-325",
    versionWords("Undid the restore. '{paper}' is back as it was before."),
    MODIFIED,
    [...ASK_TODAY, CONFIRM, UNDO_DONE, CONFIRM],
  ),
  // the version restored to has left the matter's window by the time the line is said
  state("UC-428", versionWords("Restored '{paper}'."), { ...MODIFIED, forgets: true }, [
    ...ASK_EARLIER,
    CONFIRM,
  ]),
];
