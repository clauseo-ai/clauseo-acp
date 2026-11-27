// UC's entries for the handover of a conversation from Claude to ChatGPT (host #1349's "handoff" view entry): the
// divider just before the question it went with, given (UC-1512), pending with its "Passing on" mark (UC-1406),
// not confirmed (UC-1514), not got with the next message to carry it (UC-1515) or not (UC-1516), a newer status
// (UC-1517), too long to hand over (UC-1513); what was passed on, opened (UC-1401..1404); a switch back (UC-1510)
// and what it passed on (UC-1511). The real room over
// FakeTransport, three questions with the handover before the third.
import { agentText, step, userMessage, uuid } from "../../testing/stream-script.ts";
import type {
  ErrorCode,
  HandoffEntryStatus,
  StepEntry,
  ViewEntry,
} from "../../transport/protocol.ts";
import type { Entry } from "../entry.ts";
import { threadHost } from "../hosts.ts";
import { RoomClicking, RoomOver } from "../states.tsx";
import { threadWords } from "../words.ts";

/** One exchange of its own turn: the question, the answer (none for a turn that failed first), the turn's end. */
function exchange(
  k: number,
  question: string,
  answer: string,
  end: Partial<StepEntry> = {},
): ViewEntry[] {
  const turn = uuid(k, 4);
  const q = userMessage(k * 100, k, question);
  const a = agentText(k * 100 + 10, k, answer);
  return [
    { user_message: { ...q.user_message!, turn } },
    ...(answer === "" ? [] : [{ agent_text: { ...a.agent_text!, turn } }]),
    step(k * 100 + 90, { turn, ...end }),
  ];
}

const handoff = (status: HandoffEntryStatus, resumed: boolean): ViewEntry => ({
  handoff: {
    ...(resumed ? { resumed } : {}),
    order: 305,
    handoff: uuid(1, 12),
    message: uuid(3, 5),
    from: {
      agent: "claude-acp",
      model: { id: "opus", name: "Opus 5.5" },
      effort: { id: "high", name: "High" },
    },
    to: { agent: "codex-acp", model: { id: "gpt-6.2-sol", name: "6.2 Sol" } },
    answers_covered: 2,
    items_included: 9,
    items_left_out: 3,
    status,
    ts: Date.UTC(2026, 10, 25, 11, 5),
  },
});

const FIRST = exchange(
  1,
  "Read the lease deed and the notice to quit. Was the notice served within the period the lease asks for, and does " +
    "it name the right premises?",
  "The lease (clause 14) asks for **three months'** written notice. The notice to quit is dated 2 January 2024 and " +
    "asks the tenant to leave by 31 March 2024, a little under three months. It names the shop on the ground floor, " +
    "which matches Schedule A.",
);
const SECOND = exchange(
  2,
  "Does the written statement deny receiving the notice, or only that it was valid?",
  "Only that it was valid. Paragraph 6 admits the notice reached the tenant on 4 January 2024.",
);
const THIRD_Q =
  "Going by the rent receipts, was the tenant in arrears when the notice went out? List the months, if any.";

function entries(status: HandoffEntryStatus, resumed: boolean, failure: ErrorCode): ViewEntry[] {
  const third =
    status !== "pending" && status !== "delivered"
      ? exchange(3, THIRD_Q, "", { stop: "failed", failure })
      : status === "pending"
        ? exchange(3, THIRD_Q, "").slice(0, 1)
        : exchange(3, THIRD_Q, "Yes. No rent was paid for October, November and December 2023.");
  return [...FIRST, ...SECOND, ...third, handoff(status, resumed)];
}

/** The room with the handover at `status` (a string: "brand_new" stands for one from a newer Clauseo), the thread on
 * `agent`; `resumed` for a switch back; the third turn's `failure` when it failed. */
const host =
  (
    status: string,
    agent = "codex-acp",
    resumed = false,
    failure: ErrorCode = "agent.answer_failed",
  ) =>
  () =>
    threadHost({
      entries: entries(status as HandoffEntryStatus, resumed, failure),
      agent,
      ...(status === "pending" ? { status: "running" as const } : {}),
    });
const opened = (resumed = false) => (
  <RoomClicking
    host={host("delivered", "codex-acp", resumed)}
    clicks={['[data-testid="handoff-more"]']}
  />
);

const ENTRIES: Entry[] = [
  {
    id: "UC-1512",
    words: threadWords("Switched to {agent}"),
    screen: "room/thread",
    render: () => <RoomOver host={host("delivered")} />,
  },
  {
    id: "UC-1406",
    words: threadWords("Passing on"),
    screen: "room/thread",
    render: () => <RoomOver host={host("pending")} />,
  },
  {
    id: "UC-1514",
    words: threadWords(
      "Clauseo couldn't confirm {agent} got the conversation so far, and sends it again by itself if {agent} starts afresh, so there's nothing to do.",
    ),
    screen: "room/thread",
    render: () => <RoomOver host={host("unconfirmed")} />,
  },
  {
    id: "UC-1513",
    words: threadWords("The conversation stayed with {agent}."),
    screen: "room/thread",
    render: () => (
      <RoomOver host={host("failed", "claude-acp", false, "prompt.handoff_too_long")} />
    ),
  },
  {
    id: "UC-1510",
    words: threadWords("Back to {agent}, which picks up its own conversation"),
    screen: "room/thread",
    render: () => <RoomOver host={host("delivered", "codex-acp", true)} />,
  },
  {
    id: "UC-1511",
    words: threadWords("Only what {agent} hadn't seen."),
    screen: "room/thread",
    render: () => opened(true),
  },
  {
    id: "UC-1515",
    words: threadWords(
      "{agent} didn't get the conversation so far, so it goes with your next message.",
    ),
    screen: "room/thread",
    render: () => <RoomOver host={host("failed")} />,
  },
  {
    id: "UC-1516",
    words: threadWords(
      "{agent} didn't get the conversation so far with this question; your next message goes as normal.",
    ),
    screen: "room/thread",
    render: () => <RoomOver host={host("failed", "claude-acp")} />,
  },
  {
    id: "UC-1517",
    words: threadWords(
      "Clauseo can't tell whether {agent} got the conversation so far; your next message goes as normal.",
    ),
    screen: "room/thread",
    render: () => <RoomOver host={host("brand_new")} />,
  },
  {
    id: "UC-1401",
    words: threadWords("What was passed on"),
    screen: "room/thread",
    render: () => opened(),
  },
  {
    id: "UC-1402",
    words: threadWords("Earlier answers: {n}"),
    screen: "room/thread",
    render: () => opened(),
  },
  {
    id: "UC-1403",
    words: threadWords("Passed on in full: {n}"),
    screen: "room/thread",
    render: () => opened(),
  },
  {
    id: "UC-1404",
    words: threadWords("Left out or shortened for lack of room: {n}"),
    screen: "room/thread",
    render: () => opened(),
  },
];
export default ENTRIES;
