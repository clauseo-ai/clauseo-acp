// ERROR_WORDS' entries for the matter.* codes (words-errors.ts), moved here as they were to keep that file under the
// size limit: the matter folder's refusals and the write-back's failures. Their Hindi is in words-errors.hi.ts with
// the rest.
import type { ErrorKey } from "./words-errors.ts";
import { REFUSED } from "./words-errors-refused.ts";
import { WRITTEN } from "./words-errors-written.ts";

/** The matter.* codes' words, spread into ERROR_WORDS in their place. */
export const MATTER_WORDS = {
  "matter.checkpoint_failed":
    "Clauseo couldn't save a version of this matter's papers, so this change can't be undone.",
  "matter.folder_missing":
    "This matter's folder can't be reached. It may have moved, or its drive or network isn't connected.",
  "matter.folder_not_a_matter":
    "This folder doesn't hold this matter's Clauseo file. Locate the matter's folder to carry on.",
  // settled (orchestrator, 2026-11-10); the app's Open Privacy settings button and its line are the room's
  "matter.folder_permission": "Clauseo needs permission to open this folder.",
  "matter.folder_refused": "A whole drive, your home folder or a system folder can't be a matter.",
  "matter.codex_folder":
    "This matter's folder holds a .codex folder. Rename or remove it to use ChatGPT here.",
  "matter.file_locked": "A paper is open in another program. Close it, then try again.",
  // the folder refused a change to a file; its line is picked from the error's files and refusal (errorWordsOf,
  // words-errors-refused.ts). Draft English (Rohan to approve); the Hindi is to come
  "matter.file_refused": REFUSED.only.permissions,
  // writing a file in place stopped partway; its line is picked from the error's files and write failure (errorWordsOf,
  // words-errors-written.ts). Draft English (Rohan to approve); the Hindi is to come
  "matter.file_write_failed": WRITTEN.damaged,
  // several conversations of a matter may answer at once (24 Nov): no one answer is named
  "matter.turn_running": "Wait for the answers in this matter to finish.",
  // folder PR 2 (approved 2026-11-11): each folder refusal says its reason; a folder nested with a matter, here where
  // no name is at hand, and the rare folder_is_matter take the line of a folder Clauseo can't make a matter of
  "matter.folder_is_drive": "This is the top of a drive. Choose a folder on it.",
  "matter.folder_is_home": "This is your home folder. Choose a folder inside it.",
  "matter.folder_is_system": "This is a system folder. Choose another folder.",
  "matter.folder_unreadable": "Clauseo can't read that folder.",
  "matter.folder_is_matter": "Clauseo can't make a matter of this folder. Choose another folder.",
  "matter.folder_in_matters_root":
    "That folder is where Clauseo keeps matters. Choose the folder for one matter.",
  "matter.folder_in_clauseo_data":
    "This folder is part of Clauseo's own data. Choose another folder.",
  "matter.folder_inside_matter":
    "Clauseo can't make a matter of this folder. Choose another folder.",
  "matter.folder_holds_matter":
    "Clauseo can't make a matter of this folder. Choose another folder.",
  "matter.folder_network":
    "ChatGPT can't work on a network drive. Move the matter to this computer.",
  "matter.folder_no_permissions":
    "ChatGPT can't work on a drive without file permissions. Move the matter to this computer.",
  // also a turn refused in an open matter, so not folder PR 2's choose-another line (that is matter-form-check.ts's)
  "matter.folder_too_deep":
    "This matter's folder is too deep inside other folders for Clauseo to work in it on Windows. Move it nearer the top of its drive.",
} as const satisfies Record<string, ErrorKey>;
