// matter-exists' copy review entries, apart from registry.tsx, as registry-u6.tsx is: the New matter form's Open {name}
// for a folder nested with a matter on the list, and its lines for Clauseo's own matters folder
// (matter.folder_in_matters_root), each over the form the host refuses in (hosts.ts's refusingHost); and the first
// screen's line while the folder picked is made a matter. ME-01 and ME-02, retired: "This folder is already a matter"
// is never said (folder PR 2). ME-06 to ME-12, retired: each refusal names the folder picked
// (registry-uc-folder-refusals.tsx, UC-262 to UC-271); their lines stay only where no folder is at hand to name.
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

/** Use a new folder, then Create matter, in the default location. */
const CREATE: readonly Step[] = [
  ...NEW_MATTER,
  { press: "matter-made" },
  { press: "create-matter" },
];

/** matter-exists' entries: "ME-01" and so on, each id written as it is. */
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
      // no folder to name: a pick that reads as nothing (the default location's new folder is named)
      <RoomAfter
        host={() => refusingHost("matter.folder_in_matters_root", "")}
        steps={CHANGE_CREATE}
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
  // ME-06 to ME-12, retired (folder refusals): UC-262 to UC-271 name the folder
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
