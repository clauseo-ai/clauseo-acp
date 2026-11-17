// UC's entries for a step that asks a helper (UC-906, UC-907): Claude's Agent or Task, which claude-agent-acp reports as
// kind think with the helper's description as its title, said as "Asked a helper: {task}." (done) or "Tried to ask a
// helper: {task}." (failed), in place of "Thought it through.". The real room over FakeTransport.
import type { ToolCallEntry, ToolCallStatus, ViewEntry } from "../transport/protocol.ts";
import { step, toolCall, userMessage } from "../testing/stream-script.ts";
import type { Entry } from "./entry.ts";
import { threadHost, type ThreadState } from "./hosts.ts";
import { RoomClicking } from "./states.tsx";
import { stepWords } from "./words.ts";

const QUESTION = userMessage(5, 1, "Compare the termination clauses in these contracts.");

/** Claude's Agent tool, as claude-agent-acp reports it: the kind think, the helper's description as its title. */
function helper(status: ToolCallStatus): ViewEntry {
  const entry: ToolCallEntry = {
    ...toolCall(10, 590, status).tool_call!,
    kind: "think",
    title: "Compare termination clauses",
    tool_name: "Agent",
  };
  return { tool_call: entry };
}

const host = (state: ThreadState) => () => threadHost(state);
/** The settled thread with this call folded, its group opened to the row. */
const opened = (c: ViewEntry) => () => (
  <RoomClicking
    host={host({ entries: [QUESTION, c, step(91)] })}
    clicks={['[data-testid="step-group"] button']}
  />
);

export const UC_HELPER: Entry[] = [
  {
    id: "UC-906",
    words: stepWords("Asked a helper: {task}."),
    screen: "room/thread",
    render: opened(helper("completed")),
  },
  {
    id: "UC-907",
    words: stepWords("Tried to ask a helper: {task}."),
    screen: "room/thread",
    render: opened(helper("failed")),
  },
];
