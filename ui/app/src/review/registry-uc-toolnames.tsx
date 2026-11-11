// UC's entries for a step or a permission prompt the agent named only by a tool's or a permission's name
// ("SandboxNetworkAccess", "Bash", "write_file"; room/thread/tool-names.ts): what Clauseo asks to do, beneath "Clauseo
// asks before going on.", and a network step's row in each tense. The real room over FakeTransport.
import type { ToolCallEntry, ToolCallStatus, ToolKind, ViewEntry } from "../transport/protocol.ts";
import { permission, step, toolCall, userMessage } from "../testing/stream-script.ts";
import type { Entry } from "./entry.ts";
import { threadHost, type ThreadState } from "./hosts.ts";
import { RoomClicking, RoomOver } from "./states.tsx";
import { stepLiveWords, stepWords, threadWords } from "./words.ts";

const QUESTION = userMessage(5, 1, "Check the court's cause list for tomorrow.");

/** A call titled only `title`, of `kind`, as call `n`. */
function named(title: string, kind: ToolKind, status: ToolCallStatus, n = 90): ViewEntry {
  const entry: ToolCallEntry = { ...toolCall(10, 500 + n, status).tool_call!, kind, title };
  return { tool_call: entry };
}

const host = (state: ThreadState) => () => threadHost(state);
/** A request named only `title` waiting on the lawyer. */
const asking = (title: string, kind: ToolKind) => () => (
  <RoomOver
    host={host({
      entries: [QUESTION, named(title, kind, "pending"), permission(40, 590)],
      status: "needs_you",
    })}
  />
);
const opened = (status: ToolCallStatus) => () => (
  <RoomClicking
    host={host({ entries: [QUESTION, named("SandboxNetworkAccess", "other", status), step(91)] })}
    clicks={['[data-testid="step-group"] button']}
  />
);

export const UC_TOOLNAMES: Entry[] = [
  {
    id: "UC-160",
    words: threadWords("Clauseo asks to let a command reach the internet."),
    screen: "room/thread",
    render: asking("SandboxNetworkAccess", "other"),
  },
  {
    id: "UC-161",
    words: threadWords("Clauseo asks to run a task."),
    screen: "room/thread",
    render: asking("Bash", "execute"),
  },
  {
    id: "UC-162",
    words: threadWords("Clauseo asks to read a paper."),
    screen: "room/thread",
    render: asking("read_file", "read"),
  },
  {
    id: "UC-163",
    words: threadWords("Clauseo asks to change a paper."),
    screen: "room/thread",
    render: asking("write_file", "edit"),
  },
  {
    id: "UC-164",
    words: threadWords("Clauseo asks to search the papers."),
    screen: "room/thread",
    render: asking("Grep", "search"),
  },
  {
    id: "UC-165",
    words: threadWords("Clauseo asks to look online."),
    screen: "room/thread",
    render: asking("mcp__browser__navigate", "fetch"),
  },
  {
    id: "UC-166",
    words: stepWords("Let a task reach the internet."),
    screen: "room/thread",
    render: opened("completed"),
  },
  {
    id: "UC-167",
    words: stepWords("Tried to let a task reach the internet."),
    screen: "room/thread",
    render: opened("failed"),
  },
  {
    id: "UC-168",
    words: stepLiveWords("Letting a task reach the internet."),
    screen: "room/thread",
    render: () => (
      <RoomOver
        host={host({
          entries: [QUESTION, named("SandboxNetworkAccess", "other", "in_progress")],
          status: "running",
        })}
      />
    ),
  },
];
