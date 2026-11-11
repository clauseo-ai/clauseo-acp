// matter-exists' copy review entries, apart from registry.tsx, as registry-u6.tsx is: the New matter form's lines for a
// folder nested with a matter on the list (matter.folder_inside_matter, matter.folder_holds_matter), named, with Open
// {name}; for Clauseo's own matters folder (matter.folder_in_matters_root) and folder PR 2's other refusals, each over
// the form the host refuses in (hosts.ts's refusingHost); and the first screen's line while the folder picked is made
// a matter. ME-01 and ME-02, retired: "This folder is already a matter" is never said (folder PR 2); a folder nested
// with a matter the list doesn't have, and matter.folder_is_matter, say U2-31's line.
import type { Entry } from "./entry.ts";
import { RoomAfter } from "./states.tsx";
import { MATTER, NEW_MATTER, refusingHost, type Step } from "./hosts.ts";
import type { NextErrorCode } from "../room/host-next.ts";
import { render } from "./registry-ud1-scene.tsx";
import { matterWords, type WordsRef } from "./words.ts";

/** Choose folder…, then Create matter: the picked folder refused. */
const CHANGE_CREATE: readonly Step[] = [
  ...NEW_MATTER,
  { press: "matter-existing" },
  { press: "create-matter" },
];

/** Use a new folder, then Create matter, in the default location: the folder is in the form's own words, so the line
 * names none. */
const CREATE: readonly Step[] = [
  ...NEW_MATTER,
  { press: "matter-made" },
  { press: "create-matter" },
];

/** matter-exists' entries: "ME-01" and so on, each id written as it is. */
/** Choose folder…, then Create matter, the picked folder refused with `code`. */
const refused = (id: string, words: WordsRef, code: NextErrorCode, display: string): Entry => ({
  id,
  words,
  screen: "matters/new-matter",
  render: () => <RoomAfter host={() => refusingHost(code, display)} steps={CHANGE_CREATE} />,
});

/** Use a new folder, then Create matter: refused as nested with matter A, on the list. */
const nested = (id: string, words: WordsRef, code: NextErrorCode): Entry => ({
  id,
  words,
  screen: "matters/new-matter",
  render: () => (
    <RoomAfter host={() => refusingHost(code, "", true, MATTER.matter)} steps={CREATE} />
  ),
});

export const ME: Entry[] = [
  {
    id: "ME-03",
    words: matterWords("{path} is the folder Clauseo keeps matters in."),
    screen: "matters/new-matter",
    render: () => (
      <RoomAfter
        host={() => refusingHost("matter.folder_in_matters_root", "Documents/Clauseo")}
        steps={CHANGE_CREATE}
      />
    ),
  },
  {
    id: "ME-04",
    words: matterWords("That folder is where Clauseo keeps matters."),
    screen: "matters/new-matter",
    render: () => (
      <RoomAfter
        host={() => refusingHost("matter.folder_in_matters_root", "", true)}
        steps={CREATE}
      />
    ),
  },
  {
    id: "ME-05",
    words: matterWords("Choose the folder for one matter."),
    screen: "matters/new-matter",
    render: () => (
      <RoomAfter
        host={() => refusingHost("matter.folder_in_matters_root", "Documents/Clauseo")}
        steps={CHANGE_CREATE}
      />
    ),
  },
  refused(
    "ME-06",
    matterWords("This is the top of a drive. Choose a folder on it."),
    "matter.folder_is_drive",
    "/Volumes/Case",
  ),
  refused(
    "ME-07",
    matterWords("This is your home folder. Choose a folder inside it."),
    "matter.folder_is_home",
    "/Users/rmehra",
  ),
  refused(
    "ME-08",
    matterWords("This is a system folder. Choose another folder."),
    "matter.folder_is_system",
    "/System",
  ),
  refused(
    "ME-09",
    matterWords("This folder is part of Clauseo's own data. Choose another folder."),
    "matter.folder_in_clauseo_data",
    "Library/Application Support/Clauseo",
  ),
  refused(
    "ME-10",
    matterWords("This folder is too deeply nested for Windows. Choose a shorter path."),
    "matter.folder_too_deep",
    "C:\\Users\\rmehra\\Documents\\Matters\\2026\\Acme Traders\\Pleadings",
  ),
  nested(
    "ME-11",
    matterWords("This folder is inside the folder of {name}."),
    "matter.folder_inside_matter",
  ),
  nested(
    "ME-12",
    matterWords("This folder holds the folder of {name}."),
    "matter.folder_holds_matter",
  ),
  nested("ME-13", matterWords("Open {name}"), "matter.folder_inside_matter"),
  // the first screen, Open the folder pressed and a folder picked, while the host makes it a matter (it never answers)
  {
    id: "ME-14",
    words: matterWords("Opening the folder…"),
    screen: "first-launch/welcome",
    render: render({
      first: true,
      events: [],
      clicks: ['[data-testid="open-folder"]'],
      open: {
        pickFolder: () => ({ folder: "review-folder", display: "Documents/Acme" }),
        dispatch: () => new Promise(() => {}),
      },
    }),
  },
];
