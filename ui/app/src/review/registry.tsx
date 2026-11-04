// Every string the copy review shows, one entry each. A brief that adds or changes a string adds its entry here: the
// next free id for the brief ("U1-04"), the words file and English key (words.ts's helpers), the screen, the state
// that shows it (the real screen over a scripted host, built in the render function), and, where the default desktop
// window (entry.ts) doesn't suit, the width and height to render at. Approvals live in ui/app/copy-approvals.json.
import { LocateGate, MovedFolder } from "../room/moved.tsx";
import { ScratchLine } from "../room/scratch-line.tsx";
import type { Entry } from "./entry.ts";
import { H11 } from "./registry-h11.tsx";
import { U4 } from "./registry-u4.tsx";
import { U4B } from "./registry-u4b.tsx";
import { U10 } from "./registry-u10.tsx";
import { U11 } from "./registry-u11.tsx";
import { U12 } from "./registry-u12.tsx";
import { U16A_RAIL } from "./registry-u16a-rail.tsx";
import { U16A_FIND } from "./registry-u16a-find.tsx";
import { U8 } from "./registry-u8.tsx";
import { U9 } from "./registry-u9.tsx";
import { U6 } from "./registry-u6.tsx";
import { U7 } from "./registry-u7.tsx";
import { C117 } from "./registry-c117.tsx";
import { U6F } from "./registry-u6f.tsx";
import { UTC } from "./registry-utc.tsx";
import {
  MATTER,
  NEW_MATTER,
  railHost,
  refusingHost,
  roomHost,
  scratchHost,
  type Step,
  threadHost,
  type ThreadState,
} from "./hosts.ts";
import { RoomAfter, RoomAsking, RoomOver } from "./states.tsx";
import {
  errorWords,
  frameWords,
  limitWords,
  matterFormWords,
  matterWords,
  shared,
  threadWords,
  type WordsRef,
} from "./words.ts";
import type { NextErrorCode } from "../room/host-next.ts";
import { ERROR_WORDS, type ErrorKey } from "@clauseo/components/lib/words-errors.ts";
import { agentText, step, toolCall, TURN, userMessage, uuid } from "../testing/stream-script.ts";
import type { ErrorCode, StepEntry, ViewEntry } from "../transport/protocol.ts";

/* ---------- U3: the live thread's states ---------- */

/** A local time, `days` from today. */
const atLocal = (days: number, h: number, m: number) => {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + days, h, m).getTime();
};
/** The review's turn, asked at 4:31 pm today and ended at 4:32 pm: the question's "You, 4:31 pm", and the answer's
 * "Clauseo, 4:32 pm" once its step is in. */
const ASKED = atLocal(0, 16, 31);
const ENDED = atLocal(0, 16, 32);

const asked = userMessage(5, 1, "What does clause nine of the lease say about notice?");
const QUESTION: ViewEntry = { user_message: { ...asked.user_message!, ts: ASKED } };
const ANSWER = agentText(
  6,
  1,
  "Clause nine requires **notice in writing**, served at the tenant's registered office, at least thirty days ahead.",
);
const PARTIAL = agentText(6, 1, "Clause nine requires **notice in writing**, served at");
const ended = (over: Partial<StepEntry> = {}): ViewEntry => step(9, { ts: ENDED, ...over });
const paper = (undone: boolean): ViewEntry =>
  ended({
    files: [
      {
        path: "Notice to the bank.docx",
        change: "modified",
        size_bytes: 1200,
        may_include_lawyer_edits: false,
      },
    ],
    checkpoint_after: uuid(1, 9),
    undone,
  });
const content = (block: object): ViewEntry => ({
  agent_content: {
    order: 7,
    event: uuid(1, 12),
    turn: TURN,
    message: uuid(1, 6),
    kind: "answer",
    block: block as never,
  },
});
const limit = (reason: string, resets_at?: number): ViewEntry => ({
  limit_stop: {
    order: 9,
    turn: TURN,
    reason: reason as never,
    ts: ENDED,
    ...(resets_at === undefined ? {} : { resets_at }),
  },
});

const thread = (state: ThreadState) => () => <RoomOver host={() => threadHost(state)} />;

/** One entry per error code, "U3-err-<code>", refused on a send: an id that names its code never moves when a code
 * is added. Codes whose sentence is U3-07's (words-thread.ts's) need no entry of their own. */
const ERRORS: Entry[] = (Object.entries(ERROR_WORDS) as [ErrorCode, ErrorKey][]).flatMap(
  ([code, key]) =>
    key === "The answer stopped. Your question and the answer so far are kept."
      ? []
      : [
          {
            id: `U3-err-${code}`,
            words: errorWords(key),
            screen: "room/composer",
            render: () => (
              <RoomAsking
                host={() => threadHost({ entries: [QUESTION, ANSWER, ended()], refuse: code })}
                question="And clause ten?"
              />
            ),
          },
        ],
);

/** U3's entries, each id written as it is: an id never moves when an entry is added or taken out. */
const U3: Entry[] = [
  {
    id: "U3-01",
    words: frameWords("Answering."),
    screen: "room/composer",
    render: thread({ entries: [QUESTION, PARTIAL], status: "running" }),
  },
  {
    id: "U3-02",
    words: threadWords("Failed"),
    screen: "room/thread",
    render: thread({ entries: [QUESTION, toolCall(7, 1, "failed"), ANSWER, ended()] }),
  },
  {
    id: "U3-03",
    words: threadWords("Stopped"),
    screen: "room/thread",
    render: thread({
      entries: [QUESTION, PARTIAL, toolCall(7, 1, "cancelled"), ended({ stop: "cancelled" })],
    }),
  },
  {
    id: "U3-04",
    words: threadWords("You stopped the answer. The answer so far is kept."),
    screen: "room/thread",
    render: thread({ entries: [QUESTION, PARTIAL, ended({ stop: "cancelled" })] }),
  },
  {
    id: "U3-05",
    words: threadWords("The answer stopped at its limit, before it finished."),
    screen: "room/thread",
    render: thread({ entries: [QUESTION, PARTIAL, ended({ stop: "max_tokens" })] }),
  },
  {
    id: "U3-06",
    words: threadWords("This question was declined, so there is no answer."),
    screen: "room/thread",
    render: thread({ entries: [QUESTION, ended({ stop: "refusal" })] }),
  },
  {
    id: "U3-07",
    words: threadWords("The answer stopped. Your question and the answer so far are kept."),
    screen: "room/thread",
    render: thread({
      entries: [QUESTION, PARTIAL, ended({ stop: "failed", failure: "agent.exited" })],
    }),
  },
  {
    id: "U3-08",
    words: threadWords("Your question"),
    screen: "room/thread",
    render: thread({ entries: [QUESTION, ANSWER, ended()] }),
  },
  {
    id: "U3-09",
    words: threadWords("Clauseo's answer"),
    screen: "room/thread",
    render: thread({ entries: [QUESTION, ANSWER, ended()] }),
  },
  {
    id: "U3-10",
    words: threadWords("Attached: {name}"),
    screen: "room/thread",
    render: thread({
      entries: [
        QUESTION,
        content({ resource_link: { uri: "file:///matter/Lease.pdf", name: "Lease.pdf" } }),
        ended(),
      ],
    }),
  },
  {
    id: "U3-11",
    words: threadWords("Something here can't be shown in the thread."),
    screen: "room/thread",
    render: thread({
      entries: [
        QUESTION,
        content({ audio: { data: { inline: "AAAA" }, mime_type: "audio/wav" } }),
        ended(),
      ],
    }),
  },
  {
    id: "U3-12",
    words: threadWords("Clauseo changed 1 paper."),
    screen: "room/thread",
    render: thread({ entries: [QUESTION, ANSWER, paper(false)] }),
  },
  {
    id: "U3-13",
    words: threadWords("You undid Clauseo's changes to 1 paper."),
    screen: "room/thread",
    render: thread({ entries: [QUESTION, ANSWER, paper(true)] }),
  },
  {
    id: "U3-14",
    words: limitWords("Resets {time}"),
    screen: "room/thread",
    render: thread({ entries: [QUESTION, PARTIAL, limit("plan_window", atLocal(0, 23, 59))] }),
  },
  {
    id: "U3-15",
    words: limitWords("Resets {day} {time}"),
    screen: "room/thread",
    render: thread({ entries: [QUESTION, PARTIAL, limit("plan_window", atLocal(2, 9, 0))] }),
  },
  {
    id: "U3-17",
    words: limitWords("Try again"),
    screen: "room/composer",
    // a code the wire calls retryable, since Try again shows only for one (U10)
    render: thread({ entries: [QUESTION, PARTIAL], lose: "store.io", loseRetryable: true }),
  },
  {
    id: "U3-18",
    words: limitWords("Usage limit reached"),
    screen: "room/thread",
    render: thread({ entries: [QUESTION, PARTIAL, limit("workspace_credits", atLocal(2, 9, 0))] }),
  },
  // U3-16 was "Limit reached. Reset not given.", the toolbar's sentence, which the limit row no longer says
  {
    id: "U3-48",
    words: limitWords("Reset not given."),
    screen: "room/thread",
    render: thread({ entries: [QUESTION, PARTIAL, limit("plan_window")] }),
  },
  {
    id: "U3-49",
    words: limitWords("The limit reset at {time}."),
    screen: "room/thread",
    // midnight today: always gone by, and always today
    render: thread({ entries: [QUESTION, PARTIAL, limit("plan_window", atLocal(0, 0, 0))] }),
  },
  {
    id: "U3-50",
    words: limitWords("The limit reset on {day} at {time}."),
    screen: "room/thread",
    render: thread({ entries: [QUESTION, PARTIAL, limit("plan_window", atLocal(-2, 9, 0))] }),
  },
];

/* ---------- U2: the rail and matters ---------- */

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
  ...U3,
  ...ERRORS,
  ...U4,
  ...U4B,
  ...U10,
  ...U11,
  ...U12,
  ...U16A_RAIL,
  ...U16A_FIND,
  ...U8,
  ...U9,
  ...H11,
  ...U6,
  ...U7,
  ...UTC,
  ...C117,
  ...U6F,
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
