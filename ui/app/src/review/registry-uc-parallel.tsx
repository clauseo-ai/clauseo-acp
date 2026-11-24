// UC's entries for parallel answers (issue 15, the UI side, 24 Nov): the conversations of a matter answer at the same
// time, so a message waits only while an undo, a restore or a move holds the matter, when the host queues its turn
// (`queued { behind }`, behind the conversation's own thread for a restore or a move). The real room over U7's host:
// the open conversation and "Draft petition" both waiting, as a restore holds the matter.
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
import type { ThreadStatus, ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { heldHost } from "./registry-u7.tsx";
import { RoomOver } from "./states.tsx";
import { matterWords } from "./words.ts";

const HELD = { queued: { behind: THREAD } };
/** Both conversations' messages wait for a restore: the composer's working line and card, the other's rail row. */
const held = () => <RoomOver host={() => heldHost(HELD, HELD)} />;

const LATER = "And does the renewal letter change the sublet clause?";
/** The later message, waiting or stopped before its turn started. */
const later = (status: "sent" | "removed"): ViewEntry => {
  const e = userMessage(4, 2, LATER);
  return { user_message: { ...e.user_message!, status, turn: uuid(2, 4) } };
};
const headed = (status: ThreadStatus): ViewEntry => {
  const head = header();
  head.header!.matter = MATTER.matter;
  head.header!.status = status;
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
          event(
            batch(1, [reset(window, headed({ queued: { behind: THREAD } }))], undefined, stream),
          ),
        );
        setTimeout(() => {
          const bodies = [upsert(later("removed")), upsert(headed("idle"))];
          transport.deliver(event(batch(2, bodies, undefined, stream)));
        }, 50);
      }),
  });
  return transport;
}

export const UC_PARALLEL: Entry[] = [
  {
    id: "UC-1145",
    words: matterWords("Waiting to answer."),
    screen: "matters/other",
    render: held,
  },
  {
    id: "UC-1146",
    words: matterWords(
      "An undo, restore or move is changing this matter's papers. Your message is queued and sends once that's done.",
    ),
    screen: "matters/other",
    render: held,
  },
  {
    id: "UC-1147",
    words: matterWords(
      "Not sent, because you stopped it before it started. Your message is back in the box below.",
    ),
    screen: "matters/other",
    render: () => <RoomOver host={stoppedHost} />,
  },
];
