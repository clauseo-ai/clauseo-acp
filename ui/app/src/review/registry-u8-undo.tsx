// U8's entries for an undo that kept a file because Clauseo had no earlier copy of it (the
// step_undone marker's unversioned), apart from registry-u8.tsx to keep that file under the size
// limit.
import type { Entry } from "./entry.ts";
import { undoneTurn, type Matter } from "./registry-u8.tsx";
import { versionWords } from "./words.ts";

const NOTICE = "Notice to the bank.docx";
const file = (path: string) => ({
  path,
  change: "modified" as const,
  size_bytes: 1200,
  may_include_lawyer_edits: false,
});

/**  The turn three days ago changed three files; undoing it, Clauseo had no earlier copy of two of
 * them: one only in OneDrive before, one over the size cap. */
const ANNEX = "Annexure 3.pdf";
const STATEMENT = "Bank statement 2025.pdf";
const UNDO_KEEPS: Matter = {
  files: [file(NOTICE), file(ANNEX), file(STATEMENT)],
  unversioned: [
    { path: ANNEX, reason: "cloud_only" },
    { path: STATEMENT, reason: "over_size_cap" },
  ],
  place: { synced: { service: "one_drive" } },
};
/**  The turn changed one file, only online before in a folder no named service syncs: the undo puts
 * none back. */
const ONLY_ONLINE: Matter = {
  files: [file(ANNEX)],
  unversioned: [{ path: ANNEX, reason: "cloud_only" }],
  place: "local",
};

export const U8_UNDO: Entry[] = [
  // after an undo, a file it kept because Clauseo had no earlier copy: one only in OneDrive before,
  // one over the size cap, and one only online in a folder no named service syncs (the band's
  // lines, and the change note's)
  undoneTurn(
    "U8-37",
    versionWords(
      "Clauseo can't undo the change to {file}. It was only in {service} before, so there's no earlier copy here. {service}'s version history has the earlier one.",
    ),
    UNDO_KEEPS,
  ),
  undoneTurn(
    "U8-38",
    versionWords(
      "Clauseo can't undo the change to {file}. It was only online before, so there's no earlier copy here. Your cloud service's version history may have the earlier one.",
    ),
    ONLY_ONLINE,
  ),
  undoneTurn(
    "U8-39",
    versionWords(
      "Clauseo can't undo the change to {file}. It didn't keep a copy of it before, so there's no earlier copy to bring back.",
    ),
    UNDO_KEEPS,
  ),
];
