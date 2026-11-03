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
import type { Lang } from "@clauseo/components/lib/display.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import {
  asideScene,
  chatgptScene,
  heldRestartScene,
  heldScene,
  queueHost,
  queueScene,
  requeuedScene,
  sentNowScene,
  steerMarkScene,
  steerWords,
  type QueueScene,
} from "./queue-hosts.ts";
import { RoomTyped } from "./room-typed.tsx";
import { RoomAsking, RoomClicking, RoomOver, RoomPressing } from "./states.tsx";
import { matterWords, threadWords } from "./words.ts";

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

/** Part 2's states (queue-hosts.tsx), each over its own host. */
const over = (scene: QueueScene) => <RoomOver host={() => queueHost(scene)} />;
const pressing = (scene: QueueScene, press: string) => (
  <RoomPressing host={() => queueHost(scene)} press={press} />
);
const strip = (lang: Lang) => over(queueScene(lang, 3));
const steerClaude = (lang: Lang) => (
  <RoomTyped host={() => queueHost(queueScene(lang, 0))} words={steerWords.claude(lang)} />
);
const steerChatgpt = (lang: Lang) => (
  <RoomTyped host={() => queueHost(chatgptScene(lang))} words={steerWords.chatgpt(lang)} />
);

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
  // part 2: the queue's strip over the composer (matters/queue, queue-full, held, held-failed)
  {
    id: "U7-05",
    words: matterWords("1 queued, sent once this answer is done."),
    screen: "matters/queue",
    render: (lang) => over(queueScene(lang, 1)),
  },
  {
    id: "U7-06",
    words: matterWords("{n} queued, sent in order."),
    screen: "matters/queue",
    render: strip,
  },
  {
    id: "U7-07",
    words: matterWords("The queue holds {n}."),
    screen: "matters/queue-full",
    render: (lang) => over(queueScene(lang, 10)),
  },
  {
    id: "U7-08",
    words: matterWords("{n} more"),
    screen: "matters/queue",
    render: strip,
  },
  {
    id: "U7-09",
    words: matterWords("Show fewer"),
    screen: "matters/queue",
    render: (lang) => (
      <RoomClicking host={() => queueHost(queueScene(lang, 3))} clicks={[".queue-fold"]} />
    ),
  },
  {
    id: "U7-10",
    words: matterWords("Queued messages"),
    screen: "matters/queue",
    render: strip,
  },
  {
    id: "U7-11",
    words: matterWords("Edit"),
    screen: "matters/queue",
    render: strip,
  },
  {
    id: "U7-12",
    words: matterWords("Remove"),
    screen: "matters/queue",
    render: strip,
  },
  {
    id: "U7-13",
    words: matterWords("Send now"),
    screen: "matters/queue",
    render: strip,
  },
  {
    id: "U7-14",
    words: matterWords("Edit queued message {n}"),
    screen: "matters/queue",
    render: (lang) => pressing(queueScene(lang, 3), "queued-edit"),
  },
  {
    id: "U7-15",
    words: matterWords("Save"),
    screen: "matters/queue",
    render: (lang) => pressing(queueScene(lang, 3), "queued-edit"),
  },
  {
    id: "U7-16",
    words: matterWords("That message had already gone."),
    screen: "matters/queue",
    render: (lang) =>
      pressing(
        queueScene(lang, 3, { refuse: { remove_queued_prompt: "thread.turn_not_queued" } }),
        "queued-remove",
      ),
  },
  {
    id: "U7-17",
    words: matterWords(
      "The queue is held. You stopped the answer at {time}, so nothing more is sent until you send.",
    ),
    screen: "matters/held",
    render: (lang) => over(heldScene(lang, "stopped")),
  },
  {
    id: "U7-18",
    words: matterWords(
      "The queue is held. The answer failed at {time}, so nothing more is sent until you send.",
    ),
    screen: "matters/held-failed",
    render: (lang) => over(heldScene(lang, "failed")),
  },
  {
    id: "U7-19",
    words: matterWords("Send next"),
    screen: "matters/held",
    render: (lang) => over(heldScene(lang, "stopped")),
  },
  // part 2: steering the running answer (matters/steer-claude, steer-chatgpt, steer-aside)
  {
    id: "U7-20",
    words: matterWords("Send now gives your words to Clauseo while it writes."),
    screen: "matters/steer-claude",
    render: steerClaude,
  },
  {
    id: "U7-21",
    words: matterWords(
      "ChatGPT can't take words mid-answer. Send now stops this answer, keeps what it wrote, and sends your words first.",
    ),
    screen: "matters/steer-chatgpt",
    render: steerChatgpt,
  },
  {
    id: "U7-22",
    words: matterWords("You, {time}, while Clauseo answered"),
    screen: "matters/steer-claude",
    render: (lang) => over(steerMarkScene(lang)),
  },
  {
    id: "U7-23",
    words: matterWords("Stopped at {time} to send your words first. The answer so far is kept."),
    screen: "matters/steer-chatgpt",
    render: (lang) => over(sentNowScene(lang)),
  },
  {
    id: "U7-24",
    words: matterWords("Your words came as the answer ended, so they go next."),
    screen: "matters/steer-claude",
    render: (lang) => over(requeuedScene(lang)),
  },
  {
    id: "U7-25",
    words: matterWords(
      "Your words at {time} came in before a task had finished. It went on in the background and finished at {done}. The answer uses what it found.",
    ),
    screen: "matters/steer-aside",
    render: (lang) => over(asideScene(lang)),
  },
  {
    id: "U7-26",
    words: matterWords("In the background"),
    screen: "matters/steer-aside",
    render: (lang) => over(asideScene(lang)),
  },
  {
    id: "U7-27",
    words: matterWords("Show what it found"),
    screen: "matters/steer-aside",
    render: (lang) => over(asideScene(lang)),
  },
  // part 2, after the review's first round: the held queue with no stop to name (matters/held-restart), and the
  // exchange's sections' names, which the review marks on the sections they name
  {
    id: "U7-28",
    words: matterWords("{n} queued."),
    screen: "matters/held-restart",
    render: (lang) => over(heldRestartScene(lang, 3)),
  },
  {
    id: "U7-29",
    words: matterWords("1 queued."),
    screen: "matters/held-restart-one",
    render: (lang) => over(heldRestartScene(lang, 1)),
  },
  {
    id: "U7-30",
    words: threadWords("Your question, {time}"),
    screen: "matters/held-restart",
    render: (lang) => over(heldRestartScene(lang, 3)),
  },
  {
    id: "U7-31",
    words: threadWords("Clauseo's answer, {time}"),
    screen: "matters/held-restart",
    render: (lang) => over(heldRestartScene(lang, 3)),
  },
];
