// UC's entries for a case-law step's cost (UC-488 to UC-490): the credits the Indian Kanoon server reported for that call,
// after the row's done or tried line and a space, or that nothing was charged for a call it refused. The real room over
// FakeTransport, the step group opened so the row shows.
import type { ToolCallEntry, ToolCallStatus, ViewEntry } from "../transport/protocol.ts";
import { step, toolCall, userMessage } from "../testing/stream-script.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomClicking } from "./states.tsx";
import { stepWords } from "./words.ts";

const QUESTION = userMessage(5, 1, "Find the cases on the basic structure doctrine.");

/** Claude's case-law search, ended `status`, with the cost the host joined to it. */
function search(status: ToolCallStatus, charge: NonNullable<ToolCallEntry["charge"]>): ViewEntry {
  const entry: ToolCallEntry = {
    ...toolCall(10, 590, status).tool_call!,
    kind: "other",
    title: "mcp__case-law__search_law",
    tool_name: "mcp__case-law__search_law",
    raw_input: { inline: JSON.stringify({ query: "basic structure doctrine" }) },
    charge,
  };
  return { tool_call: entry };
}

const opened = (c: ViewEntry) => () => (
  <RoomClicking
    host={() => threadHost({ entries: [QUESTION, c, step(91)] })}
    clicks={['[data-testid="step-group"] button']}
  />
);

export const UC_CASE_LAW_COST: Entry[] = [
  {
    id: "UC-488",
    words: stepWords("· {n} credits"),
    screen: "room/thread",
    render: opened(search("completed", { credits: 40, refused: false })),
  },
  {
    id: "UC-489",
    words: stepWords("· 1 credit"),
    screen: "room/thread",
    render: opened(search("completed", { credits: 1, refused: false })),
  },
  {
    id: "UC-490",
    words: stepWords("· nothing charged"),
    screen: "room/thread",
    render: opened(search("failed", { credits: 0, refused: true })),
  },
];
