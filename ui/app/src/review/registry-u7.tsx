// U7's copy review entries: apart from registry.tsx, as registry-u6.tsx is. The wait over the composer while another
// conversation of the matter holds it (matters/other), in the real room over a host whose matter has two
// conversations: the open one, and "Draft petition", which runs. Before the open one sends, Ask raises the wait, which
// offers Stop the other and Queue; once its message waits behind the other (`queued { behind }`), Stop the other
// alone, and the composer's working line says the same wait.
import {
  batch,
  event,
  header,
  reset,
  THREAD,
  userMessage,
  uuid,
} from "../testing/stream-script.ts";
import type { ThreadStatus } from "../transport/protocol.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { RoomAsking, RoomOver } from "./states.tsx";
import { matterWords } from "./words.ts";

/** The conversation that holds the matter; its id sorts below THREAD's, so the open one is THREAD. */
const OTHER = uuid(0, 1);

/** A host whose matter has THREAD, with one question asked, in `status`, and "Draft petition", running. */
function heldHost(status: ThreadStatus): FakeTransport {
  const head = header();
  head.header!.matter = MATTER.matter;
  head.header!.status = status;
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
              status,
              title: "Reply to notice",
            },
            {
              thread: OTHER,
              matter: MATTER.matter,
              agent: "claude-acp",
              status: "running",
              title: "Draft petition",
            },
          ],
        };
      throw new Error(`the review's host doesn't answer ${JSON.stringify(query)}`);
    },
    dispatch: () => "done",
    subscribe: (stream) =>
      queueMicrotask(() =>
        transport.deliver(
          event(
            batch(
              1,
              [reset([userMessage(2, 1, "What does the reply to the notice admit?")], head)],
              undefined,
              stream,
            ),
          ),
        ),
      ),
  });
  return transport;
}

/** Before sending: the open conversation idle, the other running, and Ask pressed with words, which raises the wait. */
const before = () => (
  <RoomAsking host={() => heldHost("idle")} question="Does the reply admit the notice?" />
);
/** After sending: the open conversation's message waits behind the other. */
const queued = () => <RoomOver host={() => heldHost({ queued: { behind: OTHER } })} />;

/** U7's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const U7: Entry[] = [
  {
    id: "U7-01",
    words: matterWords("Waiting for '{title}' to finish."),
    screen: "matters/other",
    render: queued,
  },
  {
    id: "U7-02",
    words: matterWords(
      "One conversation changes a matter's papers at a time. Your message is ready to go.",
    ),
    screen: "matters/other",
    render: queued,
  },
  {
    id: "U7-03",
    words: matterWords("Stop the other"),
    screen: "matters/other",
    render: queued,
  },
  {
    id: "U7-04",
    words: matterWords("Queue"),
    screen: "matters/other",
    render: before,
  },
];
