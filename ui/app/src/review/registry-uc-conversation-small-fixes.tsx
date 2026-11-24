// UC's entries for a chained command (`&&`, `||`, `;` or `|`), counted rather than named by its first program
// (UC-300 to UC-302): its activity row tried or running, and its permission prompt. Done, its row says
// "Ran {n} commands." (UC-1108). The real room over FakeTransport.
import type { ToolCallEntry, ToolCallStatus, ViewEntry } from "../transport/protocol.ts";
import { permission, step, toolCall, userMessage } from "../testing/stream-script.ts";
import type { Entry } from "./entry.ts";
import { threadHost, type ThreadState } from "./hosts.ts";
import { RoomClicking, RoomOver } from "./states.tsx";
import { stepLiveWords, stepWords, threadWords } from "./words.ts";

const QUESTION = userMessage(5, 1, "Put a copy of the notice in a drafts folder.");
const CHAINED = "New-Item -ItemType Directory drafts; Copy-Item 'Legal notice.docx' drafts";

/** The chained command as a task, in `status`. */
function task(status: ToolCallStatus): ViewEntry {
  const entry: ToolCallEntry = {
    ...toolCall(10, 590, status).tool_call!,
    kind: "execute",
    title: "",
    raw_input: { inline: JSON.stringify({ command: ["pwsh", "-c", CHAINED] }) },
  };
  return { tool_call: entry };
}

const host = (state: ThreadState) => () => threadHost(state);

export const UC_CONVERSATION_SMALL_FIXES: Entry[] = [
  {
    id: "UC-300",
    words: stepWords("Tried to run {n} commands."),
    screen: "room/thread",
    render: () => (
      <RoomClicking
        host={host({ entries: [QUESTION, task("failed"), step(91)] })}
        clicks={['[data-testid="step-group"] button']}
      />
    ),
  },
  {
    id: "UC-301",
    words: stepLiveWords("Running {n} tasks."),
    screen: "room/thread",
    render: () => (
      <RoomOver host={host({ entries: [QUESTION, task("in_progress")], status: "running" })} />
    ),
  },
  {
    id: "UC-302",
    words: threadWords("Run {n} tasks."),
    screen: "room/thread",
    render: () => (
      <RoomOver
        host={host({
          entries: [QUESTION, task("pending"), permission(11, 590)],
          status: "needs_you",
        })}
      />
    ),
  },
];
