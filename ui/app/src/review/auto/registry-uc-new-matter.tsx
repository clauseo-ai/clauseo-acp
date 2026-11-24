// UC's entries for New matter's first question (UC-1030 to UC-1036): New folder or Existing folder, each with its
// line on what it means for the lawyer's files; the existing folder's form, Change… and its line that nothing is
// moved; and the new folder's row where the host has no place for new matters. The real room over FakeTransport
// (registry-nm.tsx), with a folder under a Windows lawyer's home.
import type { Entry } from "../entry.ts";
import { EXISTING_PICKED, NEW_MATTER_ASKED, type Step } from "../hosts.ts";
import { formHost } from "../registry-nm.tsx";
import { RoomAfter } from "../states.tsx";
import { matterFormWords, type WordsRef } from "../words.ts";
import type { MatterFormKey } from "@clauseo/components/lib/words-matter-form.ts";

/** A folder under a Windows lawyer's home, as the desktop writes it: the form shows its folders in order. */
const WINDOWS_FOLDER = "Documents\\Clients\\Health Insurance Claim Denial";

/** New folder chosen on a host with no place for new matters: the row asks where. */
const NOWHERE: readonly Step[] = [...NEW_MATTER_ASKED, { press: "matter-made" }];

const entry = (
  id: string,
  key: MatterFormKey,
  steps: readonly Step[],
  place: string | null = "Documents\\Clauseo",
): Entry => {
  const words: WordsRef = matterFormWords(key);
  return {
    id,
    words,
    screen: "matters/new-matter",
    render: () => <RoomAfter host={formHost(WINDOWS_FOLDER, place)} steps={steps} />,
  };
};

const UC_NEW_MATTER: Entry[] = [
  entry("UC-1030", "New folder", NEW_MATTER_ASKED),
  entry(
    "UC-1031",
    "Clauseo makes an empty folder for this matter. Add papers to it any time.",
    NEW_MATTER_ASKED,
  ),
  entry("UC-1032", "Existing folder", NEW_MATTER_ASKED),
  entry(
    "UC-1033",
    "Use a folder that already holds the papers. It stays where it is, and nothing is moved or copied.",
    NEW_MATTER_ASKED,
  ),
  entry("UC-1034", "Change…", EXISTING_PICKED),
  entry("UC-1035", "The folder stays where it is. Nothing is moved or copied.", EXISTING_PICKED),
  entry("UC-1036", "Choose where it goes.", NOWHERE, null),
];

export default UC_NEW_MATTER;
