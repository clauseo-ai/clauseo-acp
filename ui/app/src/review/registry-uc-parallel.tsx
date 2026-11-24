// UC's entries for parallel answers (issue 15, the UI side, 24 Nov): the conversations of a matter answer at the same
// time, so a message waits only while an undo, a restore or a move holds the matter, when the host queues its turn
// (`queued { behind }`, behind the conversation's own thread for a restore or a move). The real room over U7's host:
// the open conversation and "Draft petition" both waiting, as a restore holds the matter.
import { THREAD } from "../testing/stream-script.ts";
import type { Entry } from "./entry.ts";
import { heldHost } from "./registry-u7.tsx";
import { RoomOver } from "./states.tsx";
import { matterWords } from "./words.ts";

const HELD = { queued: { behind: THREAD } };
/** Both conversations' messages wait for a restore: the composer's working line and card, the other's rail row. */
const held = () => <RoomOver host={() => heldHost(HELD, HELD)} />;

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
];
