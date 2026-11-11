// UTC's entries for tasks and the finished answer (D-016, D-017, D-026, D-027, D-028), apart from registry-utc.tsx to
// keep that file under the size limit: a task told by the program it ran and the paper it ran on, its command folded,
// a search of one site, a task's permission prompt, and the answer's Copy (Try again is the room's own words). The
// real room over FakeTransport, each state pressed as the lawyer would. Every path is made up, under a made-up matter.
import type { ToolCallEntry, ToolCallStatus, ToolKind, ViewEntry } from "../transport/protocol.ts";
import { agentText, permission, step, toolCall, userMessage } from "../testing/stream-script.ts";
import type { Entry } from "./entry.ts";
import { threadHost, type ThreadState } from "./hosts.ts";
import { RoomClicking, RoomOver } from "./states.tsx";
import { stepLiveWords, stepWords, threadWords, type WordsRef } from "./words.ts";
import { tr, type ToolResultKey } from "@clauseo/components/lib/words-tool-result.ts";

/** A local time today. */
const atLocal = (h: number, m: number) => {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate(), h, m).getTime();
};
const QUESTION: ViewEntry = (() => {
  const asked = userMessage(5, 1, "What does the legal notice ask of the client?");
  return { user_message: { ...asked.user_message!, ts: atLocal(16, 24) } };
})();
const ANSWER = agentText(
  90,
  1,
  "The notice asks for payment within fifteen days.\n\n```\nSection 138, Negotiable Instruments Act\n```",
);

/** A reasoning summary as ChatGPT sends it, opening the answer: its title in Markdown, after blank lines (D-018,
 * D-030). */
const THOUGHT = agentText(
  15,
  2,
  "\n\n**India Code link unverified**\n\nThe India Code page for section 138 didn't load, so the answer cites the bare act.",
  "thought",
);

/** A made-up matter's folder, as the agent writes its paths. */
const MATTER = "/Users/lawyer/Documents/Clauseo/Sharma";
const PYTHON = {
  command: [
    "/bin/zsh",
    "-lc",
    `python3 -c "import fitz; print(fitz.open('${MATTER}/papers/Legal notice.pdf')[0].get_text())"`,
  ],
  cwd: MATTER,
};
const SWIFT = { command: `swift ${MATTER}/scripts/count.swift` };

let next = 0;
/** A call of `kind` with this raw input, a minute apart from the last. */
function call(kind: ToolKind, status: ToolCallStatus, raw: object, n = 0): ViewEntry {
  next += 1;
  const at = n === 0 ? next : n;
  const started_at = atLocal(16, 24) + next * 60_000;
  const ends = status !== "pending" && status !== "in_progress";
  const entry: ToolCallEntry = {
    ...toolCall(10 + next, 500 + at, status).tool_call!,
    kind,
    title: "",
    raw_input: { inline: JSON.stringify(raw) },
    started_at,
    ...(ends ? { finished_at: started_at + 40_000 } : {}),
    ...(status === "failed"
      ? { raw_output: { inline: JSON.stringify({ stderr: "Error: file is password-protected" }) } }
      : {}),
  };
  return { tool_call: { ...entry, order: 10 + next } };
}

const host = (state: ThreadState) => () => threadHost(state);
const settled = (calls: ViewEntry[], end: ViewEntry = step(91, { ts: atLocal(16, 32) })) =>
  host({ entries: [QUESTION, ...calls, ANSWER, end] });
/** The settled thread, its group opened and then `then` pressed. */
const opened =
  (calls: ViewEntry[], ...then: string[]) =>
  () => (
    <RoomClicking host={settled(calls)} clicks={['[data-testid="step-group"] button', ...then]} />
  );
const running = (calls: ViewEntry[]) => () => (
  <RoomOver host={host({ entries: [QUESTION, ...calls], status: "running" })} />
);
const toolResult = (key: ToolResultKey): WordsRef => ({
  file: "lib/words-tool-result.ts",
  key,
  text: (lang) => tr(lang, key),
  later: tr.hi[key] === null,
});
const SITE = { query: "site:indiacode.nic.in Negotiable Instruments Act section 138" };
const PRESS = '[data-testid="task-command-press"]';
/** A task waiting on the lawyer's permission, its call `n`. */
const asking = (raw: object) => () => {
  const c = call("execute", "pending", raw, 90);
  return (
    <RoomOver host={host({ entries: [QUESTION, c, permission(40, 590)], status: "needs_you" })} />
  );
};

export const UTC_TASKS: Entry[] = [
  // a task named by the program and the paper it ran on, where the agent gave no words of its own (D-017)
  {
    id: "UTC-89",
    words: stepWords("Ran {program} on {name}."),
    screen: "room/thread",
    render: opened([call("execute", "completed", PYTHON)]),
  },
  {
    id: "UTC-90",
    words: stepWords("Ran {program}."),
    screen: "room/thread",
    render: opened([call("execute", "completed", { command: "ls -la" })]),
  },
  {
    id: "UTC-91",
    words: stepWords("Tried to run {program} on {name}."),
    screen: "room/thread",
    // failed: its one line of why under the words
    render: opened([call("execute", "failed", PYTHON)]),
  },
  {
    id: "UTC-92",
    words: stepWords("Tried to run {program}."),
    screen: "room/thread",
    render: opened([call("execute", "failed", { command: "ls -la" })]),
  },
  {
    id: "UTC-93",
    words: stepLiveWords("Running {program} on {name}."),
    screen: "room/thread",
    render: running([call("execute", "in_progress", PYTHON)]),
  },
  {
    id: "UTC-94",
    words: stepLiveWords("Running {program}."),
    screen: "room/thread",
    render: running([call("execute", "in_progress", SWIFT)]),
  },
  // the command, folded under the row's words, its paths cut to the matter (D-017, D-046)
  {
    id: "UTC-95",
    words: toolResult("Show the command"),
    screen: "room/thread",
    render: opened([call("execute", "completed", PYTHON)]),
  },
  {
    id: "UTC-96",
    words: toolResult("Hide the command"),
    screen: "room/thread",
    render: opened([call("execute", "completed", PYTHON)], PRESS),
  },
  // a web search of one site: the site said apart from the words (D-028)
  {
    id: "UTC-97",
    words: stepWords("Searched {site} for '{query}'."),
    screen: "room/thread",
    render: opened([call("fetch", "completed", SITE)]),
  },
  {
    id: "UTC-98",
    words: stepWords("Tried to search {site} for '{query}'."),
    screen: "room/thread",
    render: opened([call("fetch", "failed", SITE)]),
  },
  {
    id: "UTC-99",
    words: stepLiveWords("Searching {site} for '{query}'."),
    screen: "room/thread",
    render: running([call("fetch", "in_progress", SITE)]),
  },
  // a task's permission prompt in plain words, its command folded (D-016)
  {
    id: "UTC-100",
    words: threadWords("Run {program} on {name}."),
    screen: "room/thread",
    render: asking(PYTHON),
  },
  {
    id: "UTC-101",
    words: threadWords("Run {program}."),
    screen: "room/thread",
    render: asking({ command: "ls -la" }),
  },
  // the finished answer's Copy, and its words once pressed (D-026); a code block's Copy says the same. The answer
  // opens with a thought, which stands under the meta line as its first words would (D-030)
  {
    id: "UTC-102",
    words: threadWords("Copy"),
    screen: "room/thread",
    render: () => <RoomOver host={host({ entries: [QUESTION, THOUGHT, ANSWER, step(91)] })} />,
  },
  {
    id: "UTC-103",
    words: threadWords("Copied"),
    screen: "room/thread",
    render: () => <RoomClicking host={settled([])} clicks={['[data-testid="answer-copy"]']} />,
  },
];
