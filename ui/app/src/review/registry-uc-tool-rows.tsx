// UC's entries for two activity rows in plain words (UC-240 to UC-249): a skill read as a guide, never by its internal
// name ("clauseo:legal-drafting"; room/thread/skill-words.ts), and a task named by its own title where it gave no other
// words, in place of "Ran a task."; and a step of Clauseo's case-law connector in its own words (UC-475 to UC-483),
// and a group of them counted so (UC-493 to UC-498).
// The real room over FakeTransport.
import type { ToolCallEntry, ToolCallStatus, ToolKind, ViewEntry } from "../transport/protocol.ts";
import { step, toolCall, userMessage, uuid } from "../testing/stream-script.ts";
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
/** A call of the case-law connector's `tool`: as ChatGPT names it (codex-acp: `mcp.case-law.<tool>`, execute), or as
 * Claude does (`mcp__case-law__<tool>`, other). */
const law = (agent: "chatgpt" | "claude", tool: string, status: ToolCallStatus, args: object) =>
  agent === "chatgpt"
    ? call(`mcp.case-law.${tool}`, "execute", status, { server: "case-law", tool, arguments: args })
    : call(`mcp__case-law__${tool}`, "other", status, args);
const SOUGHT = { query: "Section 138 NI Act dishonour of cheque" };

const host = (state: ThreadState) => () => threadHost(state);
const opened = (c: ViewEntry) => () => (
  <RoomClicking
    host={host({ entries: [QUESTION, c, step(91)] })}
    clicks={['[data-testid="step-group"] button']}
  />
);
/** The settled thread with these calls folded into one group, its line saying them. */
const folded = (calls: ViewEntry[]) => () => (
  <RoomOver host={host({ entries: [QUESTION, ...calls, step(91)] })} />
);
/** `n` calls of one case-law tool, done, each its own. */
const laws = (n: number, make: (status: ToolCallStatus) => ViewEntry): ViewEntry[] =>
  Array.from({ length: n }, (_, i) => {
    const c = make("completed").tool_call!;
    return { tool_call: { ...c, order: 10 + i, tool_call: uuid(600 + i, 7) } };
  });
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
  // a step of Clauseo's case-law connector (room/thread/tool-names.ts, caseLawOf), under either agent
  {
    id: "UC-475",
    words: stepLiveWords("Searching case law."),
    screen: "room/thread",
    render: running(law("chatgpt", "search_law", "in_progress", SOUGHT)),
  },
  {
    id: "UC-476",
    words: stepWords("Searched case law."),
    screen: "room/thread",
    render: opened(law("claude", "search_law", "completed", SOUGHT)),
  },
  {
    id: "UC-477",
    words: stepWords("Tried to search case law."),
    screen: "room/thread",
    render: opened(law("chatgpt", "citator", "failed", { docid: 1 })),
  },
  {
    id: "UC-478",
    words: stepLiveWords("Checking your case-law credits."),
    screen: "room/thread",
    render: running(law("claude", "account", "in_progress", {})),
  },
  {
    id: "UC-479",
    words: stepLiveWords("Getting a payment link for case-law credits."),
    screen: "room/thread",
    render: running(law("chatgpt", "recharge", "in_progress", {})),
  },
  {
    id: "UC-480",
    words: stepWords("Checked your case-law credits."),
    screen: "room/thread",
    render: opened(law("chatgpt", "account", "completed", {})),
  },
  {
    id: "UC-481",
    words: stepWords("Tried to check your case-law credits."),
    screen: "room/thread",
    render: opened(law("claude", "account", "failed", {})),
  },
  {
    id: "UC-482",
    words: stepWords("Got a payment link for case-law credits."),
    screen: "room/thread",
    render: opened(law("claude", "recharge", "completed", {})),
  },
  {
    id: "UC-483",
    words: stepWords("Tried to get a payment link for case-law credits."),
    screen: "room/thread",
    render: opened(law("chatgpt", "recharge", "failed", {})),
  },
  // the folded line of a group of case-law steps, counted by what each does, under either agent
  {
    id: "UC-493",
    words: stepWords("Searched case law once."),
    screen: "room/thread",
    render: folded(laws(1, (s) => law("chatgpt", "search_law", s, SOUGHT))),
  },
  {
    id: "UC-494",
    words: stepWords("Searched case law {n} times."),
    screen: "room/thread",
    render: folded(laws(3, (s) => law("claude", "search_law", s, SOUGHT))),
  },
  {
    id: "UC-495",
    words: stepWords("Checked your case-law credits once."),
    screen: "room/thread",
    render: folded(laws(1, (s) => law("claude", "account", s, {}))),
  },
  {
    id: "UC-496",
    words: stepWords("Checked your case-law credits {n} times."),
    screen: "room/thread",
    render: folded(laws(2, (s) => law("chatgpt", "account", s, {}))),
  },
  {
    id: "UC-497",
    words: stepWords("Got a payment link for case-law credits once."),
    screen: "room/thread",
    render: folded(laws(1, (s) => law("chatgpt", "recharge", s, {}))),
  },
  {
    id: "UC-498",
    words: stepWords("Got {n} payment links for case-law credits."),
    screen: "room/thread",
    render: folded(laws(2, (s) => law("claude", "recharge", s, {}))),
  },
];
