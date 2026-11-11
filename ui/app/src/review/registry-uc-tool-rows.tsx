// UC's entries for two activity rows in plain words (UC-240 to UC-249): a skill read as a guide, never by its internal
// name ("clauseo:legal-drafting"; room/thread/skill-words.ts), and a task named by its own title where it gave no other
// words, in place of "Ran a task.". The real room over FakeTransport.
import type { ToolCallEntry, ToolCallStatus, ToolKind, ViewEntry } from "../transport/protocol.ts";
import { step, toolCall, userMessage } from "../testing/stream-script.ts";
import type { Entry } from "./entry.ts";
import { threadHost, type ThreadState } from "./hosts.ts";
import { RoomClicking, RoomOver } from "./states.tsx";
import { stepLiveWords, stepWords } from "./words.ts";

const QUESTION = userMessage(5, 1, "Draft a reply to the bank's notice.");

/** A call titled `title`, of `kind`, with `raw` as its input. */
function call(title: string, kind: ToolKind, status: ToolCallStatus, raw?: object): ViewEntry {
  const entry: ToolCallEntry = {
    ...toolCall(10, 590, status).tool_call!,
    kind,
    title,
    ...(raw === undefined ? {} : { raw_input: { inline: JSON.stringify(raw) } }),
  };
  return { tool_call: entry };
}
const skill = (name: string, status: ToolCallStatus) =>
  call(`Load skill: ${name}`, "other", status, { skill: name });
const task = (status: ToolCallStatus) => call("Run the checks", "execute", status);

const host = (state: ThreadState) => () => threadHost(state);
const opened = (c: ViewEntry) => () => (
  <RoomClicking
    host={host({ entries: [QUESTION, c, step(91)] })}
    clicks={['[data-testid="step-group"] button']}
  />
);
const running = (c: ViewEntry) => () => (
  <RoomOver host={host({ entries: [QUESTION, c], status: "running" })} />
);

export const UC_TOOL_ROWS: Entry[] = [
  {
    id: "UC-240",
    words: stepLiveWords("Running {description}."),
    screen: "room/thread",
    render: running(task("in_progress")),
  },
  {
    id: "UC-241",
    words: stepLiveWords("Reading a guide."),
    screen: "room/thread",
    render: running(skill("firm:house-style", "in_progress")),
  },
  {
    id: "UC-242",
    words: stepLiveWords("Clauseo's drafting guide"),
    screen: "room/thread",
    render: opened(skill("clauseo:legal-drafting", "completed")),
  },
  {
    id: "UC-243",
    words: stepLiveWords("Clauseo's research guide"),
    screen: "room/thread",
    render: opened(skill("clauseo:legal-research", "completed")),
  },
  {
    id: "UC-244",
    words: stepLiveWords("Clauseo's matter brief guide"),
    screen: "room/thread",
    render: opened(skill("clauseo:matter-brief", "completed")),
  },
  {
    id: "UC-245",
    words: stepLiveWords("Clauseo's browser guide"),
    screen: "room/thread",
    render: opened(skill("clauseo:agent-browser", "completed")),
  },
  {
    id: "UC-246",
    words: stepWords("Ran {description}."),
    screen: "room/thread",
    render: opened(task("completed")),
  },
  {
    id: "UC-247",
    words: stepWords("Tried to run {description}."),
    screen: "room/thread",
    render: opened(task("failed")),
  },
  {
    id: "UC-248",
    words: stepWords("Read a guide."),
    screen: "room/thread",
    render: opened(skill("firm:house-style", "completed")),
  },
  {
    id: "UC-249",
    words: stepWords("Tried to read a guide."),
    screen: "room/thread",
    render: opened(skill("firm:house-style", "failed")),
  },
];
