// UC's entries for parallel answers (issue 15, the UI side, 24 Nov; #1302 review): the conversations of a matter answer
// at the same time, so a message waits only as the host queues it: for an undo, a restore or a move it names (#1286's
// TurnQueued.hold, read as an optional field until its bindings land), or, on today's host, behind another
// conversation's answer. The real room over U7's host.
import {
  agentText,
  batch,
  event,
  header,
  reset,
  THREAD,
  upsert,
  userMessage,
  uuid,
} from "../testing/stream-script.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { FileChange, ThreadStatus, ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { heldHost, OTHER } from "./registry-u7.tsx";
import { state } from "./registry-u8.tsx";
import { RoomOver, RoomPressing } from "./states.tsx";
import { matterWords, threadWords, versionWords } from "./words.ts";

/** Queued behind its own thread: with the header's hold (heldHost's third), what the host gives for a hold. */
const QUEUED: ThreadStatus = { queued: { behind: THREAD } };
/** Both conversations' messages wait for `hold`: the composer's working line and card, the other's rail row. */
const held = (hold: "undo" | "restore" | "relocate") => () => (
  <RoomOver host={() => heldHost(QUEUED, QUEUED, hold)} />
);
/** Today's host: the open conversation's message queued behind "Draft petition", which runs, with no hold. */
const behindOther = () => <RoomOver host={() => heldHost({ queued: { behind: OTHER } })} />;

const LATER = "And does the renewal letter change the sublet clause?";
/** The later message, waiting or stopped before its turn started. */
const later = (status: "sent" | "removed"): ViewEntry => {
  const e = userMessage(4, 2, LATER);
  return { user_message: { ...e.user_message!, status, turn: uuid(2, 4) } };
};
const headed = (status: ThreadStatus, hold?: string): ViewEntry => {
  const head = header();
  head.header!.matter = MATTER.matter;
  head.header!.status = status;
  // #1286's HeaderEntry.hold, a stand-in until that binding
  if (hold !== undefined) Object.assign(head.header!, { hold });
  return head;
};

/** A conversation with one answer whose later message waited for a restore, then was stopped before it started: the
 * host's patch marking it removed comes just after the window. */
function stoppedHost(): FakeTransport {
  const transport: FakeTransport = new FakeTransport({
    query: (query) => {
      if (query === "list_matters") return { matters: [MATTER] };
      if (typeof query === "object" && query.list_threads !== undefined)
        return {
          threads: [
            {
              thread: THREAD,
              matter: MATTER.matter,
              agent: "claude-acp",
              status: "idle",
              title: "Reply to notice",
            },
          ],
        };
      throw new Error(`the review's host doesn't answer ${JSON.stringify(query)}`);
    },
    dispatch: () => "done",
    subscribe: (stream) =>
      queueMicrotask(() => {
        const window = [
          userMessage(2, 1, "What does the reply to the notice admit?"),
          agentText(3, 1, "The reply admits the tenancy and the rent, and denies the arrears."),
          later("sent"),
        ];
        transport.deliver(
          event(batch(1, [reset(window, headed(QUEUED, "undo"))], undefined, stream)),
        );
        setTimeout(() => {
          const bodies = [upsert(later("removed")), upsert(headed("idle"))];
          transport.deliver(event(batch(2, bodies, undefined, stream)));
        }, 300);
      }),
  });
  return transport;
}

/** The turn three days ago changed the notice, which another conversation's answer also changed, and the reply. */
const paper = (path: string, also: boolean): FileChange => ({
  path,
  change: "modified",
  size_bytes: 1200,
  may_include_lawyer_edits: false,
  ...(also ? { also_changed_by: [OTHER] } : {}),
});
const SHARED = {
  files: [paper("Notice to the bank.docx", true), paper("Reply to the bank.docx", false)],
};
/** Versions open, the turn's version picked, Undo pressed: the dialog. */
const UNDOING = [
  ".room-pair button",
  '[data-testid="version"][data-checkpoint="cp2"]',
  '[data-testid="version-undo"]',
];

export const UC_PARALLEL: Entry[] = [
  {
    id: "UC-1145",
    words: matterWords("Waiting to answer."),
    screen: "matters/other",
    render: held("restore"),
  },
  {
    id: "UC-1146",
    words: matterWords(
      "This matter's folder is being moved. Your message is queued and sends once that's done.",
    ),
    screen: "matters/other",
    render: held("relocate"),
  },
  {
    id: "UC-1262",
    words: matterWords("Waiting for another answer in this matter."),
    screen: "matters/other",
    render: behindOther,
  },
  {
    id: "UC-1263",
    words: matterWords(
      "An undo is changing this matter's papers. Your message is queued and sends once it's done.",
    ),
    screen: "matters/other",
    render: held("undo"),
  },
  {
    id: "UC-1264",
    words: matterWords(
      "A restore is changing this matter's papers. Your message is queued and sends once it's done.",
    ),
    screen: "matters/other",
    render: held("restore"),
  },
  // Stop pressed in this window on the waiting message, then the host's patch marks it removed
  {
    id: "UC-1147",
    words: matterWords(
      "Not sent, because you stopped it before it started. Your message is back in the box below.",
    ),
    screen: "matters/other",
    render: () => <RoomPressing host={stoppedHost} press="cancel" />,
  },
  // the same with no Stop of this window's
  {
    id: "UC-1280",
    words: matterWords("This message wasn't sent. It's back in the box below."),
    screen: "matters/other",
    render: () => <RoomOver host={stoppedHost} />,
  },
  // 15a's also_changed_by: the undo dialog's row (kept by default), and the changed-paper card's line
  state(
    "UC-1148",
    versionWords(
      "Another conversation also changed this paper; undoing takes its change away too.",
    ),
    SHARED,
    UNDOING,
  ),
  state(
    "UC-1149",
    threadWords(
      "Another conversation also changed {papers}. Undoing this takes its change away too.",
    ),
    SHARED,
    [],
  ),
];
