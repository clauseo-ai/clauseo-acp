// UC's entries for why a message never started, as the host says it (#1314: TurnEnded.cause, the entry's `unsent`,
// stand-ins until that binding): the working line for a stop, a newer message sent first, and the answer it was for
// stopping, each with the words back in the box; and the queue's line for a copy the host marks as waiting for an
// update or a sign-in (copy_of). The real room over the parallel answers' host (registry-uc-parallel.tsx).
import { userMessage, uuid } from "../testing/stream-script.ts";
import type { ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { later, stoppedHost } from "./registry-uc-parallel.tsx";
import { RoomOver } from "./states.tsx";
import { matterWords } from "./words.ts";

const ended = (status: string, cause: string) => () => (
  <RoomOver host={() => stoppedHost(later(status, cause))} />
);

/** The copy the host queued for a message held for `cause`, marked on its own entry (#1314's copy_of, a stand-in
 * until that binding). */
const copy = (cause: "update" | "sign_in"): ViewEntry => {
  const e = userMessage(5, 3, "And does the renewal letter change the sublet clause?");
  const m = {
    ...e.user_message!,
    status: "queued" as const,
    turn: uuid(3, 4),
    copy_of: { message: uuid(2, 5), cause },
  };
  return { user_message: m };
};
/** The original held for `cause`, hidden, and its queued copy. */
const held = (cause: "update" | "sign_in") => () => (
  <RoomOver host={() => stoppedHost(later("removed", cause), [copy(cause)])} />
);

export const UC_UNSENT_CAUSE: Entry[] = [
  {
    id: "UC-1296",
    words: matterWords("Not sent, because you stopped it."),
    screen: "matters/other",
    render: ended("removed", "stopped"),
  },
  {
    id: "UC-1297",
    words: matterWords("Not sent: you sent a newer message first."),
    screen: "matters/other",
    render: ended("not_sent", "sent_now"),
  },
  {
    id: "UC-1298",
    words: matterWords("Not sent, because the answer it was for stopped."),
    screen: "matters/other",
    render: ended("not_sent", "not_sent"),
  },
  {
    id: "UC-1299",
    words: matterWords("This waits because Clauseo was updating. It goes once Clauseo is ready."),
    screen: "matters/queue",
    render: held("update"),
  },
  {
    id: "UC-1247",
    words: matterWords(
      "This waits because {agent} needed you to sign in. It goes once you're signed in.",
    ),
    screen: "matters/queue",
    render: held("sign_in"),
  },
];
