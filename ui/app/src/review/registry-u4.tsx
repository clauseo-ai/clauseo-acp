// U4's copy review entries (asking permission), apart from registry.tsx to keep that file under the size limit:
// each string over the room's thread in the state that shows it.
import {
  chosen,
  commandCall,
  editCall,
  permission,
  userMessage,
  uuid,
} from "../testing/stream-script.ts";
import type { ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost, type ThreadState } from "./hosts.ts";
import { RoomOver, RoomPressing } from "./states.tsx";
import { frameWords, shared, threadWords, versionWords } from "./words.ts";

const thread = (state: ThreadState) => () => <RoomOver host={() => threadHost(state)} />;

const TIGHTEN = userMessage(5, 1, "Tighten the notice to the bank.");
/** The thread with the lawyer's question and then `entries`, waiting on the lawyer, or with `status` as given. */
const asked = (entries: ViewEntry[], status: ThreadState["status"] = "needs_you") =>
  thread({ entries: [TIGHTEN, ...entries], status });
/** A prompt to save a new version of one paper inside the matter, all four answers offered. */
const ON_PAPER = [editCall(6, 1, "pending"), permission(7, 1)];
const pendingOnPaper = asked(ON_PAPER);
/** Allow always pressed on that prompt, its answer held while the OS asks the lawyer to confirm. */
const confirming = () => (
  <RoomPressing
    host={() => threadHost({ entries: [TIGHTEN, ...ON_PAPER], status: "needs_you", hold: true })}
    press="permission-allow_always"
  />
);
/** An elicitation the agent sent, waiting: "A question for you", its words and Stop the answer. */
const ELICITATION: ViewEntry = {
  elicitation: {
    order: 7,
    elicitation: uuid(1, 9),
    request: { url: { message: "Sign in to Westlaw", url: "https://example.com/login" } },
    state: "pending",
  },
};

/** U4's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const U4: Entry[] = [
  {
    id: "U4-01",
    words: threadWords("Don't allow for this matter"),
    screen: "room/thread",
    render: pendingOnPaper,
  },
  {
    id: "U4-02",
    words: threadWords("Clauseo asks before going on."),
    screen: "room/thread",
    render: asked([commandCall(6, 2, "pending"), permission(7, 2)]),
  },
  {
    id: "U4-03",
    words: threadWords("No longer waiting for an answer."),
    screen: "room/thread",
    render: asked(
      [editCall(6, 1, "pending"), permission(7, 1, { expired: { reason: "turn_ended" } })],
      "idle",
    ),
  },
  {
    id: "U4-04",
    words: threadWords("Allowed once."),
    screen: "room/thread",
    render: asked([commandCall(6, 2, "completed"), permission(7, 2, chosen("allow_once"))], "idle"),
  },
  {
    id: "U4-05",
    words: threadWords("Allowed for this matter."),
    screen: "room/thread",
    render: asked(
      [commandCall(6, 2, "completed"), permission(7, 2, chosen("allow_always"))],
      "idle",
    ),
  },
  {
    id: "U4-06",
    words: threadWords("Not allowed."),
    screen: "room/thread",
    render: asked([permission(7, 3, chosen("reject_once"), null)], "idle"),
  },
  {
    id: "U4-07",
    words: threadWords("Not allowed for this matter."),
    screen: "room/thread",
    render: asked([permission(7, 3, chosen("reject_always"), null)], "idle"),
  },
  // the prompt's words from before U4 (design/03-system), first on the room's screen with U4
  {
    id: "U4-08",
    words: threadWords("Clauseo wants to save a new version of {paper}."),
    screen: "room/thread",
    render: pendingOnPaper,
  },
  {
    id: "U4-09",
    words: threadWords("Allow once"),
    screen: "room/thread",
    render: pendingOnPaper,
  },
  {
    id: "U4-10",
    words: threadWords("Always for this matter"),
    screen: "room/thread",
    render: pendingOnPaper,
  },
  {
    id: "U4-11",
    words: threadWords("Don't allow"),
    screen: "room/thread",
    render: pendingOnPaper,
  },
  {
    id: "U4-12",
    words: threadWords("Allowed once. Clauseo saved a new version of {paper}."),
    screen: "room/thread",
    render: asked([editCall(6, 1, "completed"), permission(7, 1, chosen("allow_once"))], "idle"),
  },
  {
    id: "U4-13",
    words: threadWords("Allowed for this matter. Clauseo saved a new version of {paper}."),
    screen: "room/thread",
    render: asked([editCall(6, 1, "completed"), permission(7, 1, chosen("allow_always"))], "idle"),
  },
  {
    id: "U4-14",
    words: threadWords("Not allowed. Clauseo left {paper} as it was."),
    screen: "room/thread",
    render: asked([editCall(6, 1, "pending"), permission(7, 1, chosen("reject_once"))], "idle"),
  },
  {
    id: "U4-15",
    words: shared("Every change is kept."),
    screen: "room/thread",
    render: pendingOnPaper,
  },
  // the Mac's frame says macOS, the Windows frame Windows
  {
    id: "U4-16",
    words: versionWords("macOS is asking you to confirm."),
    screen: "room/thread",
    render: confirming,
  },
  {
    id: "U4-17",
    words: versionWords("Windows is asking you to confirm."),
    screen: "room/thread",
    render: confirming,
  },
  // the elicitation's row: the rail's words for a waiting question, and the composer's Stop, said in full
  {
    id: "U4-18",
    words: frameWords("A question for you"),
    screen: "room/thread",
    render: asked([ELICITATION]),
  },
  {
    id: "U4-19",
    words: threadWords("Stop the answer"),
    screen: "room/thread",
    render: asked([ELICITATION]),
  },
];
