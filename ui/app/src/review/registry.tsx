// Every string the copy review shows, one entry each. A brief that adds or changes a string adds its entry here: the
// brief's next free id, the words file and English key (words.ts's helpers), the screen, the state that shows it (the
// real screen over a scripted host), and the window's size where entry.ts's default doesn't suit. Approvals: copy-approvals/.
import { LocateGate, MovedFolder } from "../room/moved.tsx";
import { ScratchLine } from "../room/scratch-line.tsx";
import type { Entry } from "./entry.ts";
import { H11 } from "./registry-h11.tsx";
import { NATIVE } from "./registry-mn.tsx";
import { U4 } from "./registry-u4.tsx";
import { U4B } from "./registry-u4b.tsx";
import { U10 } from "./registry-u10.tsx";
import { U11 } from "./registry-u11.tsx";
import { U12 } from "./registry-u12.tsx";
import { U15 } from "./registry-u15.tsx";
import { U15_ROOM } from "./registry-u15-room.tsx";
import { U17 } from "./registry-u17.tsx";
import { LONG_QUESTION } from "./registry-long-question.tsx";
import { WEB_SETTINGS } from "./registry-web-settings.tsx";
import { U16A_RAIL } from "./registry-u16a-rail.tsx";
import { U16A_FIND } from "./registry-u16a-find.tsx";
import { ARCHIVED } from "./registry-archived.tsx";
import { U8 } from "./registry-u8.tsx";
import { U8_UNDO } from "./registry-u8-undo.tsx";
import { U9 } from "./registry-u9.tsx";
import { U6 } from "./registry-u6.tsx";
import { U7 } from "./registry-u7.tsx";
import { TF } from "./registry-follow.tsx";
import { C117 } from "./registry-c117.tsx";
import { U6F } from "./registry-u6f.tsx";
import { ME } from "./registry-me.tsx";
import { UTC } from "./registry-utc.tsx";
import { WEB_SOURCES } from "./registry-web-sources.tsx";
import { UET } from "./registry-uet.tsx";
import { UD1 } from "./registry-ud1.tsx";
import { UD2 } from "./registry-ud2.tsx";
import { UD2_PLANS } from "./registry-ud2-plans.tsx";
import { UC_ALL } from "./registry-uc-word.tsx";
import { UC_TOOLNAMES } from "./registry-uc-toolnames.tsx";
import { UC_TOOL_ROWS } from "./registry-uc-tool-rows.tsx";
import { SAC } from "./registry-sac.tsx";
import { NM } from "./registry-nm.tsx";
import { ownEntries, ownEntry } from "./registry-offline.tsx";
import { PK } from "./registry-pk.tsx";
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
  matterWords,
  shared,
  threadWords,
  type WordsRef,
} from "./words.ts";
import type { NextErrorCode } from "../room/host-next.ts";
import type { ThreadKey } from "@clauseo/components/lib/words-thread.ts";
import { ERROR_WORDS, type ErrorKey } from "@clauseo/components/lib/words-errors.ts";
import { agentText, step, toolCall, TURN, userMessage, uuid } from "../testing/stream-script.ts";
import type { ErrorCode, StepEntry, ViewEntry } from "../transport/protocol.ts";
import { U16B } from "./registry-u16b.tsx";
import { FOLDER_RIGHTS } from "./registry-folder-rights.tsx";
import { FOLDER_CHOOSER } from "./registry-folder-chooser.tsx";
import { TN } from "./registry-notices.tsx";
import { UBL } from "./registry-blocklist.tsx";
import { R23 } from "./registry-update.tsx";
import { IT_ENTRIES } from "./registry-it.tsx";

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
/** A turn's end under the answer so far: its stop, and the line that stop says. */
const stoppedAt = (id: string, key: ThreadKey, stop: StepEntry["stop"]): Entry => ({
  id,
  words: threadWords(key),
  screen: "room/thread",
  render: thread({ entries: [QUESTION, PARTIAL, ended({ stop })] }),
});

/** One entry per error code, "U3-err-<code>", refused on a send: an id that names its code never moves when a code
 * is added. Codes whose sentence is U3-07's (words-thread.ts's), or that registry-offline.tsx's ownEntry names, need
 * no entry of their own. */
const ERRORS: Entry[] = (Object.entries(ERROR_WORDS) as [ErrorCode, ErrorKey][]).flatMap(
  ([code, key]) =>
    key === "The answer stopped. Your question and the answer so far are kept." ||
    ownEntry(code, key)
      ? ownEntries(code)
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
  stoppedAt("U3-04", "You stopped the answer. The answer so far is kept.", "cancelled"),
  stoppedAt("U3-05", "The answer stopped at its limit, before it finished.", "max_tokens"),
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
  // U3-16 was "Limit reached", the toolbar's sentence, which the limit row no longer says
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
  stoppedAt(
    "U3-51",
    "You didn't allow a request, so the answer stopped. The answer so far is kept.",
    "not_allowed",
  ),
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

/** Choose folder…, then Create matter: the picked folder refused with `code`. */
const CHANGE_CREATE: readonly Step[] = [
  ...NEW_MATTER,
  { press: "matter-existing" },
  { press: "create-matter" },
];

/** A line of the new matter form's refusal (matter-form-check.ts's createErrorLine), after Choose folder… and Create,
 * or with `next` after Use a new folder and Create, in the default location, where the line names no folder. */
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
      steps={
        next ? [...NEW_MATTER, { press: "matter-made" }, { press: "create-matter" }] : CHANGE_CREATE
      }
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
  ...U15,
  ...U15_ROOM,
  ...U17,
  ...LONG_QUESTION,
  ...WEB_SETTINGS,
  ...U16A_RAIL,
  ...U16A_FIND,
  ...ARCHIVED,
  ...U8,
  ...U8_UNDO,
  ...U9,
  ...H11,
  ...NATIVE,
  ...U6,
  ...U7,
  ...TF,
  ...UTC,
  ...C117,
  ...U6F,
  ...WEB_SOURCES,
  ...UET,
  ...ME,
  ...UD1,
  ...UD2,
  ...UD2_PLANS,
  ...UC_ALL,
  ...UC_TOOLNAMES,
  ...UC_TOOL_ROWS,
  ...SAC,
  {
    id: "U2-01",
    words: frameWords("Add a matter's folder."),
    screen: "matters/rail-empty",
    ...RAIL,
    render: () => <RoomOver host={() => roomHost([])} />,
  },
  // U2-02 to U2-06, retired: the rail's date groups (Tomorrow, This week, Next week, Later, No date yet) gave way to
  // Needs you, Working and the rest by latest activity (NM-11, NM-12)
  railState("U2-07", limitWords("Limit reached until {time}")),
  railState("U2-08", limitWords("Limit reached until {day} {time}")),
  railState("U2-09", limitWords("Limit reached")),
  // U2-10 to U2-15, retired: a whole drive, the home folder and a system folder each say folder PR 2's one line
  // (ME-06 to ME-08)
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
  // U2-20, retired: the next date's line went with the field (a matter is a name, a client and a folder)
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
  // matter.folder_refused, which doesn't say why; also a folder nested with a matter the list doesn't have, and
  // matter.folder_is_matter
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
  // U2-34, retired: the Folder row's label names it for a screen reader too
  ...U16B,
  ...FOLDER_RIGHTS,
  ...FOLDER_CHOOSER,
  ...TN,
  ...UBL,
  ...R23,
  ...IT_ENTRIES,
  ...NM,
  ...PK,
];
