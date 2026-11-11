// The redesigned new matter form's and the rail's order's copy review entries (NM-01 to NM-12): the form in the real
// room over FakeTransport, opened by New matter with the name typed, as it stands with no folder, with a folder the
// picker gave (a long one, cut short in its middle), with the new folder in Clauseo's place chosen, and with Enter
// pressed before the form was ready; and the rail with Needs you, Working and the rest (hosts.ts's railHost). Apart
// from registry.tsx to keep that file under the size limit.
import { FakeTransport } from "../transport/fake-transport.ts";
import { detailed, fakeNext } from "../room/fake-next.ts";
import type { Entry } from "./entry.ts";
import { MATTER, NEW_MATTER, railHost, type Step } from "./hosts.ts";
import { RoomAfter, RoomOver } from "./states.tsx";
import { frameWords, matterFormWords, type WordsRef } from "./words.ts";

/** A folder under home, as the desktop writes it, long enough to be cut short in the dialog. */
export const LONG_FOLDER =
  "Documents/Clients/Acme Holdings Private Limited/Corporate/2026 Share purchase and disclosure schedules/Working papers";

/** A NextHost with Clauseo's place for new matters, its picker giving `display`. */
const formHost = (display: string) => () =>
  new FakeTransport({
    next: fakeNext({ matters: () => [detailed(MATTER)], location: "Documents/Clauseo" }),
    pickFolder: () => ({ folder: "review-folder", display }),
  });

const OPEN: readonly Step[] = [{ press: "new-matter" }];
const PICKED: readonly Step[] = [...NEW_MATTER, { press: "matter-existing" }];
const MADE: readonly Step[] = [...NEW_MATTER, { press: "matter-made" }];
/** Enter in the name before anything is typed: both missing fields say so. */
const TRIED: readonly Step[] = [...OPEN, { enter: 0 }];

const form = (
  id: string,
  words: WordsRef,
  steps: readonly Step[],
  display = "Documents/Clients/Acme",
): Entry => ({
  id,
  words,
  screen: "matters/new-matter",
  render: () => <RoomAfter host={formHost(display)} steps={steps} />,
});

/** The rail open beside the work, every row in view. */
const RAIL = { width: 1280, height: 1500 } as const;

export const NM: Entry[] = [
  form("NM-01", matterFormWords("Matter name"), OPEN),
  form("NM-02", matterFormWords("Client (optional)"), OPEN),
  form("NM-03", matterFormWords("Folder"), PICKED),
  form("NM-04", matterFormWords("No folder chosen"), NEW_MATTER),
  form("NM-05", matterFormWords("Choose folder…"), NEW_MATTER),
  form("NM-06", matterFormWords("Use a new folder in {folder}"), NEW_MATTER),
  form("NM-07", matterFormWords("Clauseo creates this folder. Add papers to it any time."), MADE),
  form("NM-08", matterFormWords("Name the matter."), TRIED),
  form("NM-09", matterFormWords("Choose a folder for the matter."), TRIED),
  // the long path, cut short in its middle, the whole of it under the pointer
  form("NM-10", matterFormWords("Create matter"), PICKED, LONG_FOLDER),
  {
    id: "NM-11",
    words: frameWords("Working"),
    screen: "matters/rail-states",
    ...RAIL,
    render: () => <RoomOver host={railHost} />,
  },
  {
    id: "NM-12",
    words: frameWords("Other matters"),
    screen: "matters/rail-states",
    ...RAIL,
    render: () => <RoomOver host={railHost} />,
  },
];
