// UC's entries for restoring one document from Versions (UC-316 to UC-325, UC-341, UC-392 to UC-399, and UC-428): Restore
// this document beside the
// documents that differ from now, the restore dialog it opens for that document alone, and the line once it is done,
// in the room with Versions open over registry-u8.tsx's host, after the presses that bring each state. The English
// is provisional until Rohan's review, its Hindi too.
import type { FileChange } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { state, type Matter } from "./registry-u8.tsx";
import { versionWords } from "./words.ts";

const NOTICE = "Notice to the bank.docx";
const changed = (change: FileChange["change"], path = NOTICE): FileChange => ({
  path,
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
/** The notice was renamed since: restoring it brings it back at its old name, and the renamed copy stays. */
const RENAMED: Matter = {
  compare: [changed({ renamed: { from: NOTICE } }, "Notice to the bank, sent.docx")],
  changes: [changed("added")],
};
/** The undo of a restore that brought the notice back, which wasn't in the matter before it: the undo removes it. */
const BROUGHT_BACK: Matter = { ...MODIFIED, removes: [NOTICE] };
/** The notice changed again after the restore: undoing it writes nothing. */
const EDITED_SINCE: Matter = { ...MODIFIED, undoChanges: [] };
const UNDOING_TODAY = [...ASK_TODAY, CONFIRM, UNDO_DONE, CONFIRM] as const;

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
    UNDOING_TODAY,
  ),
  // the notice changed while the dialog was open, so the dialog asked again, then the restore wrote
  state(
    "UC-392",
    versionWords(
      "'{paper}' had changed since you opened this, so Clauseo restored it as it was then. Your newer version is kept in Versions.",
    ),
    { ...MODIFIED, movesWhileOpen: true },
    [...ASK_EARLIER, CONFIRM],
  ),
  state(
    "UC-393",
    versionWords(
      "Undid the restore. '{paper}' wasn't in this matter before it, so Clauseo moved it to the Trash.",
    ),
    BROUGHT_BACK,
    UNDOING_TODAY,
  ),
  state(
    "UC-394",
    versionWords(
      "Undid the restore. '{paper}' wasn't in this matter before it, so Clauseo moved it to the Recycle Bin.",
    ),
    // said with the review's platform set to Windows; on the Mac this cell shows UC-393
    { ...BROUGHT_BACK, platform: "windows" },
    UNDOING_TODAY,
  ),
  // the bin couldn't take it (a network drive, say), so it went for good: Versions keeps a copy
  state(
    "UC-399",
    versionWords(
      "Undid the restore. '{paper}' wasn't in this matter before it, so Clauseo removed it. A copy stays in Versions.",
    ),
    { ...BROUGHT_BACK, forGood: true },
    UNDOING_TODAY,
  ),
  state(
    "UC-395",
    versionWords(
      "Nothing was undone: Clauseo couldn't safely put '{paper}' back, so it left it as it is.",
    ),
    EDITED_SINCE,
    UNDOING_TODAY,
  ),
  // the restore is refused: a folder now has the notice's name, or the folder it was in is now a file
  state(
    "UC-396",
    versionWords(
      "Clauseo can't restore this paper: there's now a folder called '{name}' where it was. Nothing has changed. Rename or move that folder, then try again.",
    ),
    { ...MODIFIED, clash: NOTICE },
    [...ASK_EARLIER, CONFIRM],
  ),
  state(
    "UC-397",
    versionWords(
      "Clauseo can't restore this paper: '{name}', the folder it was in, is now a file. Nothing has changed. Rename or move that file, then try again.",
    ),
    { ...MODIFIED, compare: [changed("modified", `Drafts/${NOTICE}`)], clash: "Drafts" },
    [...ASK_EARLIER, CONFIRM],
  ),
  state(
    "UC-398",
    versionWords(
      "'{paper}' comes back as it was then, next to '{current}', which stays as it is. You can undo this.",
    ),
    RENAMED,
    ASK_EARLIER,
  ),
  // the paper changed as Restore was pressed (saved in Word, say), so the host wrote nothing
  state(
    "UC-341",
    versionWords(
      "Nothing was restored: Clauseo couldn't safely bring back that version of '{paper}', so it left it as it is.",
    ),
    { ...MODIFIED, wroteNothing: true },
    [...ASK_EARLIER, CONFIRM],
  ),
  // the version restored to has left the matter's window by the time the line is said
  state("UC-428", versionWords("Restored '{paper}'."), { ...MODIFIED, forgets: true }, [
    ...ASK_EARLIER,
    CONFIRM,
  ]),
];
