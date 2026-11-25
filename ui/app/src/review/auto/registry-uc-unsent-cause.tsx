// UC's entries for why a message never started, as the host says it (TurnEnded.cause, the entry's `unsent`; the
// status stays removed): the working line for a newer message sent first and for
// the answer it was for having stopped, each with the words back in the box (a stop reads as UC-1147, a removal as
// UC-1280); and the queue's line for a copy the host queued for an update or a sign-in (copy_of), whose original's
// words stay out of the box. The real room over the parallel answers' host (parallel-hosts.ts).
import { userMessage, uuid } from "../../testing/stream-script.ts";
import type { UnsentCause, ViewEntry } from "../../transport/protocol.ts";
import type { Entry } from "../entry.ts";
import { later, stoppedHost } from "../parallel-hosts.ts";
import { RoomOver } from "../states.tsx";
import { matterWords } from "../words.ts";

const ended = (cause: UnsentCause) => () => (
  <RoomOver host={() => stoppedHost(later("removed", cause))} />
);

/** The copy the host queued for a message held for `cause`, marked on its own entry (copy_of). */
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

const UC_UNSENT_CAUSE: Entry[] = [
  {
    id: "UC-1297",
    words: matterWords("Not sent: you sent a newer message first. It's back in the box below."),
    screen: "matters/other",
    render: ended("sent_now"),
  },
  {
    id: "UC-1298",
    words: matterWords("Not sent: the answer it was for had stopped. It's back in the box below."),
    screen: "matters/other",
    render: ended("not_sent"),
  },
  {
    id: "UC-1299",
    words: matterWords(
      "An update was running. This message waits in the queue and goes once the update is done.",
    ),
    screen: "matters/queue",
    render: held("update"),
  },
  {
    id: "UC-1247",
    words: matterWords(
      "You were signed out of {agent}. This message waits in the queue and goes once you're signed in.",
    ),
    screen: "matters/queue",
    render: held("sign_in"),
  },
];

export default UC_UNSENT_CAUSE;
