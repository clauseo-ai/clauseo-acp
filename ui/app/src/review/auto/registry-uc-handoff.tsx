// UC's entries for the handover of a conversation from Claude to ChatGPT (host #1349's "handoff" view entry): the
// divider just before the question it went with, given (UC-1130), pending with its "Passing on" mark (UC-1406),
// failed (UC-1400) with where to act (UC-1405), and what was passed on, opened (UC-1401..1404). The real room over
// FakeTransport, three questions with the handover before the third.
import { agentText, step, userMessage, uuid } from "../../testing/stream-script.ts";
import type { HandoffEntryStatus, StepEntry, ViewEntry } from "../../transport/protocol.ts";
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

const handoff = (status: HandoffEntryStatus): ViewEntry => ({
  handoff: {
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

function entries(status: HandoffEntryStatus): ViewEntry[] {
  const third =
    status === "failed"
      ? exchange(3, THIRD_Q, "", { stop: "failed", failure: "agent.answer_failed" })
      : status === "pending"
        ? exchange(3, THIRD_Q, "").slice(0, 1)
        : exchange(3, THIRD_Q, "Yes. No rent was paid for October, November and December 2023.");
  return [...FIRST, ...SECOND, ...third, handoff(status)];
}

const host = (status: HandoffEntryStatus) => () =>
  threadHost({
    entries: entries(status),
    agent: "codex-acp",
    ...(status === "pending" ? { status: "running" as const } : {}),
  });
const opened = () => (
  <RoomClicking host={host("delivered")} clicks={['[data-testid="handoff-more"]']} />
);

const ENTRIES: Entry[] = [
  {
    id: "UC-1130",
    words: threadWords("Context handoff"),
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
    id: "UC-1400",
    words: threadWords("Context not restored"),
    screen: "room/thread",
    render: () => <RoomOver host={host("failed")} />,
  },
  {
    id: "UC-1405",
    words: threadWords("Try again sends your question with it."),
    screen: "room/thread",
    render: () => <RoomOver host={host("failed")} />,
  },
  {
    id: "UC-1401",
    words: threadWords("What was passed on"),
    screen: "room/thread",
    render: opened,
  },
  {
    id: "UC-1402",
    words: threadWords("Earlier answers: {n}"),
    screen: "room/thread",
    render: opened,
  },
  {
    id: "UC-1403",
    words: threadWords("Passed on in full: {n}"),
    screen: "room/thread",
    render: opened,
  },
  {
    id: "UC-1404",
    words: threadWords("Left out or shortened for lack of room: {n}"),
    screen: "room/thread",
    render: opened,
  },
];
export default ENTRIES;
