// UC-873's copy review entry, apart from registry.tsx to keep that file under the size limit: the working line in the real
// room over a host whose thread is Starting, with the lawyer's question asked. The line's one more line shows once the
// thread has held Starting for STARTING_HINT_MS (starting-hint.ts), so the cell waits that long for its mark.
import { userMessage } from "../testing/stream-script.ts";
import type { ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost, type ThreadState } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { frameWords } from "./words.ts";

const thread = (state: ThreadState) => () => <RoomOver host={() => threadHost(state)} />;

const ASKED: ViewEntry = {
  user_message: {
    ...userMessage(5, 1, "Summarise the notice to the bank.").user_message!,
    ts: new Date(2026, 10, 16, 10, 12).getTime(),
  },
};

/** UC-873's entries, each id written as it is. */
export const UC_STARTING: Entry[] = [
  {
    id: "UC-873",
    words: frameWords("{agent} is taking longer than usual to start."),
    screen: "room/composer",
    render: thread({ entries: [ASKED], status: "starting" }),
  },
];
