// matter-exists' copy review entries, apart from registry.tsx, as registry-u6.tsx is: the New matter form's lines for a
// folder already a matter (matter.folder_is_matter), named when the matters list has the matter, and for Clauseo's own
// matters folder (matter.folder_in_matters_root), each over the form the host refuses in (hosts.ts's refusingHost).
import type { Entry } from "./entry.ts";
import { RoomAfter } from "./states.tsx";
import { MATTER, NEW_MATTER, refusingHost, type Step } from "./hosts.ts";
import { matterWords } from "./words.ts";

/** Change, then Create the matter: the picked folder refused. */
const CHANGE_CREATE: readonly Step[] = [
  ...NEW_MATTER,
  { press: "matter-existing" },
  { press: "create-matter" },
];

/** Create the matter in the default location: the folder is in the form's own words, so the line names none. */
const CREATE: readonly Step[] = [...NEW_MATTER, { press: "create-matter" }];

/** A matter the matters list doesn't have: the folder's matter, from another install or forgotten. */
const UNKNOWN = "00000000-0000-4000-8000-000000000042";

/** matter-exists' entries: "ME-01" and so on, each id written as it is. */
export const ME: Entry[] = [
  {
    id: "ME-01",
    words: matterWords("This folder is already a matter: {name}."),
    screen: "matters/new-matter",
    render: () => (
      <RoomAfter
        host={() => refusingHost("matter.folder_is_matter", "", true, MATTER.matter)}
        steps={CREATE}
      />
    ),
  },
  {
    id: "ME-02",
    words: matterWords("This folder is already a matter."),
    screen: "matters/new-matter",
    render: () => (
      <RoomAfter
        host={() => refusingHost("matter.folder_is_matter", "", true, UNKNOWN)}
        steps={CREATE}
      />
    ),
  },
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
];
