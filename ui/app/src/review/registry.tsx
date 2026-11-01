// Every string the copy review shows, one entry each. A brief that adds or changes a string adds its entry here: the
// next free id for the brief ("U1-04"), the words file and English key (words.ts's helpers), the screen, the state
// that shows it (the real screen over a scripted host, built in the render function), and, where the default desktop
// window (entry.ts) doesn't suit, the width and height to render at. Approvals live in ui/app/copy-approvals.json.
import { LocateGate, MovedFolder } from "../room/moved.tsx";
import { ScratchLine } from "../room/scratch-line.tsx";
import type { Entry } from "./entry.ts";
import {
  MATTER,
  NEW_MATTER,
  railHost,
  refusingHost,
  roomHost,
  scratchHost,
  type Step,
} from "./hosts.ts";
import { RoomAfter, RoomOver } from "./states.tsx";
import {
  frameWords,
  limitWords,
  matterFormWords,
  matterWords,
  shared,
  type WordsRef,
} from "./words.ts";
import type { NextErrorCode } from "../room/host-next.ts";

/** The rail open beside the work, every row in view: a window past the room's 1200 px breakpoint, tall enough for
 * the rail in every state. */
const RAIL = { width: 1280, height: 1500 } as const;

/** A string of the rail in every state (hosts.ts's railHost). */
const railState = (id: string, words: WordsRef): Entry => ({
  id,
  words,
  screen: "matters/rail-states",
  ...RAIL,
  render: () => <RoomOver host={railHost} />,
});

/** Change, then Create the matter: the picked folder refused with `code`. */
const CHANGE_CREATE: readonly Step[] = [
  ...NEW_MATTER,
  { press: "matter-existing" },
  { press: "create-matter" },
];

/** A line of the new matter form's refusal (matter-form-check.ts's createErrorLine), after Change and Create, or with
 * `next` after Create into the default location, where the line names no folder. */
const refusal = (
  id: string,
  words: WordsRef,
  code: NextErrorCode,
  display: string,
  next = false,
): Entry => ({
  id,
  words,
  screen: "matters/new-matter",
  render: () => (
    <RoomAfter
      host={() => refusingHost(code, display, next)}
      steps={next ? [...NEW_MATTER, { press: "create-matter" }] : CHANGE_CREATE}
    />
  ),
});

export const REGISTRY: readonly Entry[] = [
  {
    id: "U1-01",
    words: shared("Matters"),
    screen: "room/toolbar",
    // the toolbar and the top of the room under it
    height: 200,
    render: () => <RoomOver host={() => roomHost([MATTER])} />,
  },
  {
    id: "U1-02",
    words: frameWords("No matters yet"),
    screen: "room/toolbar",
    // the toolbar and the top of the room under it
    height: 200,
    render: () => <RoomOver host={() => roomHost([])} />,
  },
  {
    id: "U1-03",
    words: shared("No papers yet."),
    screen: "room/wall",
    render: () => <RoomOver host={() => roomHost([MATTER], [])} />,
  },
  // shared with U3's working line: exactly en "Answering.", hi "उत्तर दिया जा रहा है।"
  railState("U3-01", frameWords("Answering.")),
  {
    id: "U2-01",
    words: frameWords("Add a matter's folder."),
    screen: "matters/rail-empty",
    ...RAIL,
    render: () => <RoomOver host={() => roomHost([])} />,
  },
  railState("U2-02", frameWords("Tomorrow")),
  railState("U2-03", frameWords("This week")),
  railState("U2-04", frameWords("Next week")),
  railState("U2-05", frameWords("Later")),
  railState("U2-06", frameWords("No date yet")),
  railState("U2-07", limitWords("Limit reached until {time}")),
  railState("U2-08", limitWords("Limit reached until {day} {time}")),
  railState("U2-09", limitWords("Limit reached. Reset not given.")),
  refusal(
    "U2-10",
    matterWords("{path} is a whole drive."),
    "matter.folder_is_drive",
    "/Volumes/Case",
  ),
  // the second line of a whole drive (matter.folder_refused has its own line, U2-31)
  refusal(
    "U2-11",
    matterWords("A matter is one folder of papers. Choose the folder for one matter."),
    "matter.folder_is_drive",
    "/Volumes/Case",
  ),
  refusal(
    "U2-12",
    matterWords("{path} is your home folder."),
    "matter.folder_is_home",
    "/Users/rmehra",
  ),
  refusal(
    "U2-13",
    matterWords("It holds far more than one matter. Choose the folder for one matter."),
    "matter.folder_is_home",
    "/Users/rmehra",
  ),
  refusal(
    "U2-14",
    matterWords("{path} is a folder the computer keeps for itself."),
    "matter.folder_is_system",
    "/System",
  ),
  refusal(
    "U2-15",
    matterWords("Choose the folder that holds the matter's papers."),
    "matter.folder_is_system",
    "/System",
  ),
  refusal(
    "U2-16",
    matterWords("Clauseo can't read {path}."),
    "matter.folder_unreadable",
    "Documents/Locked",
  ),
  refusal(
    "U2-17",
    matterWords("Clauseo can't read that folder."),
    "matter.folder_unreadable",
    "",
    true,
  ),
  refusal(
    "U2-18",
    matterWords("It may be locked, or on a drive that's no longer connected."),
    "matter.folder_unreadable",
    "Documents/Locked",
  ),
  refusal(
    "U2-19",
    matterWords("Clauseo couldn't create the matter. Try again, or choose another folder."),
    "internal",
    "Documents/Mehta",
  ),
  {
    id: "U2-20",
    words: matterWords("Write the date as 30 October 2026."),
    screen: "matters/new-matter",
    render: () => (
      <RoomAfter
        host={() => refusingHost("internal", "", true)}
        steps={[
          ...NEW_MATTER,
          { type: { field: 2, text: "next Tuesday" } },
          { press: "create-matter" },
        ]}
      />
    ),
  },
  {
    id: "U2-21",
    words: matterWords("A matter for quick questions, with no papers of its own."),
    screen: "matters/scratch",
    width: 640,
    height: 120,
    render: () => <ScratchLine />,
  },
  {
    id: "U2-22",
    words: matterWords("Every question asked here is kept, with its versions, like any matter's."),
    screen: "matters/scratch",
    width: 640,
    height: 120,
    render: () => <ScratchLine />,
  },
  // matters/moved: the parts U3's Work renders from folderMissing (room/moved.tsx)
  {
    id: "U2-23",
    words: matterWords("This matter's folder has moved."),
    screen: "matters/moved",
    width: 640,
    height: 320,
    render: () => <MovedFolder folder="Documents/Khan Leather Works" onLocate={() => {}} />,
  },
  {
    id: "U2-24",
    words: matterWords(
      "It was at {path}. Find it where it is now, and the matter opens as you left it.",
    ),
    screen: "matters/moved",
    width: 640,
    height: 320,
    render: () => <MovedFolder folder="Documents/Khan Leather Works" onLocate={() => {}} />,
  },
  {
    id: "U2-25",
    words: matterWords(
      "Its conversations and versions are kept on this computer, so nothing is lost while the folder is away.",
    ),
    screen: "matters/moved",
    width: 640,
    height: 320,
    render: () => <MovedFolder folder="Documents/Khan Leather Works" onLocate={() => {}} />,
  },
  {
    id: "U2-26",
    words: matterWords("Locate the folder to ask about this matter"),
    screen: "matters/moved",
    width: 640,
    height: 120,
    render: () => <LocateGate onLocate={() => {}} />,
  },
  // Find says only what it searches: names without H5's clients, names and clients with them
  {
    id: "U2-27",
    words: matterWords("Find a matter by its name"),
    screen: "matters/rail",
    ...RAIL,
    render: () => <RoomOver host={() => roomHost([MATTER])} />,
  },
  railState("U2-28", matterWords("Find a matter by its name or client")),
  {
    id: "U2-29",
    words: matterWords("Search covers names."),
    screen: "matters/rail-find",
    ...RAIL,
    render: () => <RoomAfter host={() => roomHost([MATTER])} steps={[{ find: "Kapoor" }]} />,
  },
  {
    id: "U2-30",
    words: matterWords("Search covers names and clients."),
    screen: "matters/rail-find",
    ...RAIL,
    render: () => <RoomAfter host={railHost} steps={[{ find: "Zaveri" }]} />,
  },
  // matter.folder_refused, which doesn't say why: true of a folder already a matter, an unreadable matter file, the
  // matters' own folder, a drive, the home folder and a system folder
  refusal(
    "U2-31",
    matterWords("Clauseo can't make a matter of this folder. Choose another folder."),
    "matter.folder_refused",
    "Documents/Acme",
  ),
  // the design's words (design/03-system/src/lib/words-matters.ts), approved with it
  {
    id: "U2-32",
    words: matterWords("Quick questions, kept like any matter"),
    screen: "room/toolbar",
    height: 200,
    render: () => <RoomOver host={scratchHost} />,
  },
  railState("U2-33", frameWords("Stopped on {day} at {time}")),
  // what Change changes, for a screen reader, where no folder line says it (without H20)
  {
    id: "U2-34",
    words: matterFormWords("The matter's folder"),
    screen: "matters/new-matter",
    render: () => <RoomAfter host={() => refusingHost("internal")} steps={NEW_MATTER} />,
  },
];
