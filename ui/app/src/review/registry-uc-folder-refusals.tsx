// Folder refusals' copy review entries (UC-262 to UC-277): each refused folder named as the lawyer picked it. New
// matter's lines over the form the host refuses in (hosts.ts's refusingHost), Choose folder… then Create pressed; the
// folder in use (UC-264) with Open {name} under it. Locate folder's over a room whose open matter's folder moved
// (locatingHost), Locate folder pressed, the line under the moved folder's words. They replace ME-06 to ME-12 and U2-31,
// retired: the lines that said "this folder" stay only where no folder is at hand to name.
import type { Entry } from "./entry.ts";
import { RoomAfter } from "./states.tsx";
import { MATTER, NEW_MATTER, locatingHost, refusingHost, type Step } from "./hosts.ts";
import type { NextErrorCode } from "../room/host-next.ts";
import { matterWords, type WordsRef } from "./words.ts";

/** Choose folder…, then Create matter: the picked folder refused. */
const CHANGE_CREATE: readonly Step[] = [
  ...NEW_MATTER,
  { press: "matter-existing" },
  { press: "create-matter" },
];

/** The moved folder's Locate folder, pressed. */
const LOCATE: readonly Step[] = [{ press: "moved-locate" }];

/** New matter refusing the folder `display` with `code`, naming MATTER when `named`. */
const made = (
  id: string,
  words: WordsRef,
  code: NextErrorCode,
  display: string,
  named = false,
): Entry => ({
  id,
  words,
  screen: "matters/new-matter",
  render: () => (
    <RoomAfter
      host={() => refusingHost(code, display, false, named ? MATTER.matter : undefined)}
      steps={CHANGE_CREATE}
    />
  ),
});

/** Locate folder refusing the folder `display` with `code`, naming the moved matter, MATTER or none. */
const located = (
  id: string,
  words: WordsRef,
  code: NextErrorCode,
  display: string,
  names?: "self" | "other",
): Entry => ({
  id,
  words,
  screen: "matters/moved",
  render: () => <RoomAfter host={() => locatingHost(code, display, names)} steps={LOCATE} />,
});

export const UC_FOLDER_REFUSALS: Entry[] = [
  made(
    "UC-262",
    matterWords("{path} is inside the folder of {name}. Choose another folder."),
    "matter.folder_inside_matter",
    "Documents/Acme Traders/Pleadings",
    true,
  ),
  made(
    "UC-263",
    matterWords("{path} contains the folder of {name}. Choose another folder."),
    "matter.folder_holds_matter",
    "Documents/Matters",
    true,
  ),
  made(
    "UC-264",
    matterWords("{name} already uses {path}. Open {name}, or choose another folder."),
    "matter.folder_is_matter",
    "Documents/Acme Traders",
    true,
  ),
  made(
    "UC-265",
    matterWords(
      "{path} is inside, or contains, the folder of a matter you removed from the list. Choose another folder.",
    ),
    "matter.folder_of_forgotten_matter",
    "Documents/Rao v. Union of India",
  ),
  made(
    "UC-266",
    matterWords(
      "{path} is inside, or contains, a folder Clauseo is still making into a matter. Try again in a moment, or choose another folder.",
    ),
    "matter.folder_being_made",
    "Documents/Mehta family trust",
  ),
  made(
    "UC-267",
    matterWords("{path} is the top of a drive. Choose a folder on it."),
    "matter.folder_is_drive",
    "/Volumes/Case",
  ),
  made(
    "UC-268",
    matterWords("{path} is your home folder. Choose a folder inside it."),
    "matter.folder_is_home",
    "/Users/rmehra",
  ),
  made(
    "UC-269",
    matterWords("{path} is a system folder. Choose another folder."),
    "matter.folder_is_system",
    "/System",
  ),
  made(
    "UC-270",
    matterWords("Clauseo keeps its own files in {path}. Choose another folder."),
    "matter.folder_in_clauseo_data",
    "Library/Application Support/Clauseo",
  ),
  made(
    "UC-271",
    matterWords("{path} is too deeply nested for Windows. Choose a folder with a shorter path."),
    "matter.folder_too_deep",
    "C:\\Users\\rmehra\\Documents\\Matters\\2026\\Acme Traders\\Pleadings",
  ),
  made(
    "UC-272",
    matterWords("Clauseo can't make a matter of {path}. Choose another folder."),
    "matter.folder_refused",
    "Documents/Acme",
  ),
  located(
    "UC-273",
    matterWords("{path} is already this matter's folder."),
    "matter.folder_is_matter",
    "Documents/Khan Leather Works",
    "self",
  ),
  located(
    "UC-274",
    matterWords("{name} already uses {path}. Choose this matter's own folder."),
    "matter.folder_is_matter",
    "Documents/Acme Traders",
    "other",
  ),
  located(
    "UC-275",
    matterWords("{path} isn't this matter's folder. Choose the folder you moved it to."),
    "matter.folder_not_a_matter",
    "Desktop/Khan",
  ),
  located(
    "UC-276",
    matterWords("Clauseo can't find {path}. Choose the folder again."),
    "matter.folder_missing",
    "Desktop/Khan",
  ),
  located(
    "UC-277",
    matterWords("Clauseo can't use {path} for this matter. Choose another folder."),
    "matter.folder_refused",
    "Desktop/Khan",
  ),
];
