// The redesigned new matter form's and the rail's order's copy review entries (NM-01 to NM-12): the form in the real
// room over FakeTransport, opened by New matter, New folder and the name typed, or Existing folder and the folder the
// picker gave (a long one, cut short in its middle), and with Enter pressed before the form was ready (a host with no
// place for new matters, so the folder is missing too); the first screen's form, which still offers both folders; and
// the rail with Needs you, Working and the rest (hosts.ts's railHost). Apart from registry.tsx for the size limit.
import { FakeTransport } from "../transport/fake-transport.ts";
import { detailed, fakeNext } from "../room/fake-next.ts";
import type { Entry } from "./entry.ts";
import {
  EXISTING_PICKED,
  MATTER,
  NEW_MATTER,
  NEW_MATTER_ASKED,
  railHost,
  type Step,
} from "./hosts.ts";
import { render } from "./registry-ud1-scene.tsx";
import { RoomAfter, RoomOver } from "./states.tsx";
import { frameWords, matterFormWords, type WordsRef } from "./words.ts";

/** A folder under home, as the desktop writes it, long enough to be cut short in the dialog. */
export const LONG_FOLDER =
  "Documents/Clients/Acme Holdings Private Limited/Corporate/2026 Share purchase and disclosure schedules/Working papers";

/** A NextHost with Clauseo's place for new matters (none with `place` null), its picker giving `display`. */
export const formHost =
  (display: string, place: string | null = "Documents/Clauseo") =>
  () =>
    new FakeTransport({
      next: fakeNext({ matters: () => [detailed(MATTER)], location: place }),
      pickFolder: () => ({ folder: "review-folder", display }),
    });

const PICKED = EXISTING_PICKED;
const MADE = NEW_MATTER;
/** New folder, then Enter in the name before anything is typed, with no place for new matters: both say so. */
const TRIED: readonly Step[] = [...NEW_MATTER_ASKED, { press: "matter-made" }, { enter: 0 }];

/** The first screen's New matter: its form, which still offers a picked folder or a new one. */
const firstForm = (id: string, words: WordsRef): Entry => ({
  id,
  words,
  screen: "matters/new-matter",
  render: render({ first: true, events: [], clicks: ['[data-testid="start-new-matter"]'] }),
});

const form = (
  id: string,
  words: WordsRef,
  steps: readonly Step[],
  display = "Documents/Clients/Acme",
  place: string | null = "Documents/Clauseo",
): Entry => ({
  id,
  words,
  screen: "matters/new-matter",
  render: () => <RoomAfter host={formHost(display, place)} steps={steps} />,
});

/** The rail open beside the work, every row in view. */
const RAIL = { width: 1280, height: 1500 } as const;

export const NM: Entry[] = [
  form("NM-01", matterFormWords("Matter name"), NEW_MATTER),
  form("NM-02", matterFormWords("Client (optional)"), NEW_MATTER),
  form("NM-03", matterFormWords("Folder"), PICKED),
  firstForm("NM-04", matterFormWords("No folder chosen")),
  firstForm("NM-05", matterFormWords("Choose folder…")),
  // offered only where the host has a place for new matters: the first screen's form, in the app
  firstForm("NM-06", matterFormWords("Use a new folder in {folder}")),
  form("NM-07", matterFormWords("Clauseo creates this folder. Add papers to it any time."), MADE),
  form("NM-08", matterFormWords("Name the matter."), TRIED, undefined, null),
  form("NM-09", matterFormWords("Choose a folder for the matter."), TRIED, undefined, null),
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
