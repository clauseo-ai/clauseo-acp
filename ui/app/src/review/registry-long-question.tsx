// The long question's copy review entries (question-fold.tsx): a pasted question of about two pages in the real room
// over FakeTransport, folded after eight lines with Show all (UC-142), and opened with Show less (UC-143). The text is
// the probe's fixture (long-question-fixtures.ts): fake legal text between fake parties.
import { agentText, step, userMessage } from "../testing/stream-script.ts";
import { LONG_ANSWER, longQuestionText } from "../room/long-question-fixtures.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomClicking } from "./states.tsx";
import { threadWords } from "./words.ts";

const ENTRIES = [
  userMessage(2, 1, longQuestionText("2page")),
  agentText(3, 1, LONG_ANSWER),
  step(4),
];

/** The settled thread with the long question, then `clicks` pressed in order. */
const state = (clicks: string[]) => () => (
  <RoomClicking host={() => threadHost({ entries: ENTRIES })} clicks={clicks} />
);

/** The long question's entries, each id written as it is. */
export const LONG_QUESTION: Entry[] = [
  {
    id: "UC-142",
    words: threadWords("Show all"),
    screen: "room/thread",
    render: state([]),
  },
  {
    id: "UC-143",
    words: threadWords("Show less"),
    screen: "room/thread",
    render: state([".asked-fold"]),
  },
];
