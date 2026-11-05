// Host notices' copy review entries (TN-01 on), apart from registry.tsx to keep that file under the size limit: a
// notice the host sends with its kind, in the room's thread, shown in the app's words (notice-words.ts). Each over the
// thread the notice is in, as the host sends it.
import type { ReactNode } from "react";
import { uuid, userMessage } from "../testing/stream-script.ts";
import type { ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { threadWords } from "./words.ts";

const CONTEXT =
  "Clauseo doesn't remember the earlier questions and answers in this conversation. Include what it needs from them in your next question.";

/** The lawyer's question, then the host's notice that the agent's new session doesn't remember it. */
const thread =
  (notice: ViewEntry): (() => ReactNode) =>
  () => (
    <RoomOver
      host={() =>
        threadHost({
          entries: [
            userMessage(5, 1, "What does clause nine of the lease say about notice?"),
            notice,
          ],
        })
      }
    />
  );

const CONTEXT_NOTICE: ViewEntry = {
  notice: {
    order: 6,
    event: uuid(1, 10),
    level: "info",
    text: CONTEXT,
    kind: "context_not_restored",
    ts: 6000,
  },
};

/** The notices' entries: "TN-01" and so on, each id written as it is. */
export const TN: Entry[] = [
  {
    id: "TN-01",
    words: threadWords(CONTEXT),
    screen: "room/thread",
    render: thread(CONTEXT_NOTICE),
  },
];
