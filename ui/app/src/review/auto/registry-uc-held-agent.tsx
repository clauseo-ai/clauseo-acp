// UC's entries for the other plan held for the next message (held-agent.ts): in an ongoing Claude conversation,
// ChatGPT picked in the plan menu, "ChatGPT from your next message" with its own model and effort (UC-1098), and the
// way back, "Back to Claude" (UC-1099). In the real room over registry-pk.tsx's fake host. The switch's two refusals
// (thread.background_work, prompt.handoff_too_long) are registry.tsx's ERRORS, under their codes' ids; the first's
// line while an answer runs is here, under its code's id with ".running", and while the agent waits on the lawyer's
// answer to a permission request (".permission") or to its question (".question").
import {
  BACKGROUND_PERMISSION,
  BACKGROUND_QUESTION,
  BACKGROUND_RUNNING,
} from "@clauseo/components/lib/words-errors.ts";
import { agentText, permission, userMessage } from "../../testing/stream-script.ts";
import type { ViewEntry } from "../../transport/protocol.ts";
import type { Entry } from "../entry.ts";
import { threadHost } from "../hosts.ts";
import { BOTH, ONGOING, renderPick } from "../registry-pk.tsx";
import { RoomAsking } from "../states.tsx";
import { errorWords, threadWords } from "../words.ts";

const screen = "room/composer-pickers";

/** What the latest turn ran with, as the header holds it after its start. */
const RAN = {
  model: { id: "opus-5-5", name: "Opus 5.5" },
  effort: { id: "medium", name: "Medium" },
};

/** The conversation's plan menu opened, and ChatGPT picked in it. */
const HOLD_CHATGPT = [
  { press: "button.room-status-plan" },
  { press: '[role="option"]:last-child' },
];

/** A message naming the held plan, refused with thread.background_work while Claude waits on the lawyer: a permission
 * request pending, or (none pending) its question. */
const refusedAsking = (extra: ViewEntry[]) => () => (
  <RoomAsking
    host={() =>
      threadHost({
        entries: [
          userMessage(5, 1, "Summarise the notice to the bank."),
          agentText(6, 1, "Reading the notice: it is dated 4 January 2024."),
          ...extra,
        ],
        status: "needs_you",
        refuse: "thread.background_work",
      })
    }
    question="Check the possession notice against it."
  />
);

const UC_HELD_AGENT: Entry[] = [
  // an ongoing Claude conversation, ChatGPT picked for the next message: its own model and effort beside it
  {
    id: "UC-1098",
    words: threadWords("{agent} from your next message"),
    screen,
    render: renderPick({ events: BOTH, threads: ONGOING, head: RAN, steps: HOLD_CHATGPT }),
  },
  // the same: the way back to the conversation's own plan
  {
    id: "UC-1099",
    words: threadWords("Back to {agent}"),
    screen,
    render: renderPick({ events: BOTH, threads: ONGOING, head: RAN, steps: HOLD_CHATGPT }),
  },
  // a message naming the held plan, refused while Claude's answer runs: Stop or wait
  {
    id: "U3-err-thread.background_work.running",
    words: errorWords(BACKGROUND_RUNNING),
    screen: "room/composer",
    render: () => (
      <RoomAsking
        host={() =>
          threadHost({
            entries: [
              userMessage(5, 1, "Summarise the notice to the bank."),
              agentText(6, 1, "Reading the notice: it is dated 4 January 2024."),
            ],
            status: "running",
            refuse: "thread.background_work",
          })
        }
        question="Check the possession notice against it."
      />
    ),
  },
  {
    id: "U3-err-thread.background_work.permission",
    words: errorWords(BACKGROUND_PERMISSION),
    screen: "room/composer",
    render: refusedAsking([permission(7, 1)]),
  },
  {
    id: "U3-err-thread.background_work.question",
    words: errorWords(BACKGROUND_QUESTION),
    screen: "room/composer",
    render: refusedAsking([]),
  },
];

export default UC_HELD_AGENT;
