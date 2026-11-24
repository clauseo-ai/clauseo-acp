// UC's entries for steps that name what ran (issue 13; UC-1100 to UC-1114, provisional): a tool by its readable name
// ("Used Indian Kanoon: search judgments."), a skill by its name, a command by the agent's own words for it, a chained
// command counted, a helper asked with no task, and the folded line's parts that list them in one sentence. Each over
// the steps as Claude or ChatGPT sends them (testing/step-lines.ts), in the real room over FakeTransport.
import type { ToolCallEntry, ViewEntry } from "../../transport/protocol.ts";
import { agentText, step, userMessage } from "../../testing/stream-script.ts";
import {
  commandOf,
  helperOf,
  mcpOf,
  readOf,
  noticeOf,
  searchOf,
  sharmaSteps,
  skillOf,
  stepCall,
  unknownOf,
  yieldStep,
  claudeOwn,
  heldOf,
  RULED,
  type Agent,
} from "../../testing/step-lines.ts";
import type { StepKey } from "@clauseo/components/lib/words-steps.ts";
import type { Entry } from "../entry.ts";
import { threadHost } from "../hosts.ts";
import { RoomClicking, RoomOver } from "../states.tsx";
import { stepLiveWords, stepWords } from "../words.ts";

const QUESTION = userMessage(
  5,
  1,
  "Read the plaint, the written statement and the loan agreement, and tell me whether the bank's notice under section 13(2) gave the sixty days the Act asks for.",
);

const ANSWER = agentText(
  90,
  1,
  "The notice fell short. The bank's notice under section 13(2) is dated 4 January 2024 and the bank took possession on 20 February 2024, forty-seven days later. Section 13(3A) and 13(4) of the SARFAESI Act give the borrower sixty days before the bank may act. The plaint pleads this at paragraph 14, and the written statement does not deny the dates. Indian Kanoon has Mardia Chemicals v. Union of India (2004) 4 SCC 311, which holds the sixty days mandatory. I have drafted a short reply on that footing.",
);

const rows = (calls: ToolCallEntry[]): ViewEntry[] => calls.map((tool_call) => ({ tool_call }));
const host = (calls: ToolCallEntry[]) => () =>
  threadHost({ entries: [QUESTION, ...rows(calls), ANSWER, step(91)] });
/** The group opened to its rows. */
const opened = (id: string, key: StepKey, calls: ToolCallEntry[]): Entry => ({
  id,
  words: stepWords(key),
  screen: "room/thread",
  render: () => <RoomClicking host={host(calls)} clicks={['[data-testid="step-group"] button']} />,
});
/** The group folded, its line saying the part. */
const folded = (id: string, key: StepKey, calls: ToolCallEntry[]): Entry => ({
  id,
  words: stepWords(key),
  screen: "room/thread",
  render: () => <RoomOver host={host(calls)} />,
});

const C: Agent = "Claude";
const G: Agent = "ChatGPT";
const KANOON = { query: "SARFAESI section 13(2) notice period" };
const PAGES = "Count the pages of the plaint";
const sharma = (agent: Agent) => sharmaSteps(agent).map((e) => e.tool_call!);

const UC_STEP_LINES: Entry[] = [
  // the long answer's steps on the Sharma matter, opened: papers, a search, a skill, Indian Kanoon, a command
  opened("UC-1100", "Used {tool}.", sharma(C)),
  opened("UC-1101", "Tried to use {tool}.", [
    mcpOf(G, "indian-kanoon", "search_judgments", KANOON, "failed"),
  ]),
  opened("UC-1102", "Used the {skill} skill.", [skillOf(C, "clauseo:legal-drafting")]),
  opened("UC-1103", "Tried to use the {skill} skill.", [
    skillOf(G, "clauseo:legal-research", "failed"),
  ]),
  opened("UC-1104", "Ran: {description}.", [commandOf(C, "pdfinfo Plaint.pdf", PAGES)]),
  opened("UC-1105", "Tried to run: {description}.", [
    commandOf(C, "pdfinfo Plaint.pdf", PAGES, "failed"),
  ]),
  opened("UC-1106", "Asked a helper.", [helperOf(C)]),
  opened("UC-1107", "Tried to ask a helper.", [helperOf(G, "cancelled")]),
  opened("UC-1108", "Ran {n} commands.", [
    commandOf(G, "mkdir drafts && cp 'Legal notice.docx' drafts", null),
  ]),
  folded("UC-1109", "used the {skill} skill", sharma(C)),
  folded("UC-1110", "used 1 skill", [readOf(C, "Plaint.docx"), skillOf(C, "")]),
  folded("UC-1111", "asked a helper", [readOf(G, "Plaint.docx"), helperOf(G)]),
  folded("UC-1112", "asked {n} helpers", [helperOf(C), helperOf(C)]),
  folded("UC-1113", "planned the work", [
    readOf(C, "Plaint.docx"),
    stepCall("think", "", "completed"),
  ]),
  folded("UC-1114", "used a tool", [readOf(G, "Plaint.docx"), unknownOf(G, "")]),
  // Claude's Yield: its wait for its own background work, quiet in the group
  opened("UC-1199", "Waited for the background work.", [helperOf(C), yieldStep()]),
  opened("UC-1202", "Tried to wait for the background work.", [yieldStep("failed")]),
  folded("UC-1203", "waited for the background work", [
    readOf(C, "Plaint.docx"),
    helperOf(C),
    yieldStep(),
  ]),
  {
    id: "UC-1204",
    words: stepLiveWords("Waiting for the background work."),
    screen: "room/thread",
    render: () => (
      <RoomOver
        host={() =>
          threadHost({
            entries: [QUESTION, ...rows([helperOf(C), yieldStep("in_progress")])],
            status: "running",
          })
        }
      />
    ),
  },
  // Claude's own tools by what they did; Clauseo's off-limits rule; ChatGPT's helper steps
  opened("UC-1261", "Tried to plan the work.", [claudeOwn("TodoWrite", "failed")]),
  opened("UC-1265", "Stopped background work.", [claudeOwn("TaskStop")]),
  opened("UC-1266", "Tried to stop background work.", [claudeOwn("TaskStop", "failed")]),
  opened("UC-1267", "Asked you.", [claudeOwn("AskUserQuestion")]),
  opened("UC-1268", "Tried to ask you.", [claudeOwn("AskUserQuestion", "cancelled")]),
  folded("UC-1269", "stopped background work", [readOf(C, "Plaint.docx"), claudeOwn("TaskStop")]),
  folded("UC-1270", "asked you", [readOf(C, "Plaint.docx"), claudeOwn("AskUserQuestion")]),
  opened("UC-1271", "Clauseo keeps this off limits", [heldOf("failed", RULED)]),
  folded("UC-1272", "1 kept off limits by Clauseo.", [
    readOf(C, "Plaint.docx"),
    heldOf("failed", RULED),
  ]),
  folded("UC-1273", "{n} kept off limits by Clauseo.", [
    readOf(C, "Plaint.docx"),
    heldOf("failed", RULED),
    heldOf("failed", RULED),
  ]),
  opened("UC-1274", "Waited for helpers.", [stepCall("other", "wait", "completed", { raw: {} })]),
  {
    id: "UC-1281",
    words: stepLiveWords("Waiting for helpers."),
    screen: "room/thread",
    render: () => (
      <RoomOver
        host={() =>
          threadHost({
            entries: [QUESTION, ...rows([stepCall("other", "wait", "in_progress", { raw: {} })])],
            status: "running",
          })
        }
      />
    ),
  },
  opened("UC-1282", "Worked with a helper.", [
    stepCall("other", "Start subagent extract", "completed", { raw: {} }),
    stepCall("other", "closeAgent", "completed", { raw: {} }),
  ]),
  opened("UC-1283", "Tried to work with a helper.", [
    stepCall("other", "closeAgent", "failed", { raw: {} }),
  ]),
  folded("UC-1284", "worked with a helper", [
    readOf(G, "Plaint.docx"),
    stepCall("other", "spawnAgent", "completed", { raw: {} }),
    stepCall("other", "closeAgent", "completed", { raw: {} }),
  ]),
  folded("UC-1285", "1 held back.", [
    readOf(C, "Plaint.docx"),
    heldOf("failed", undefined, "edit", "Edit"),
  ]),
  folded("UC-1286", "{n} held back.", [
    readOf(C, "Plaint.docx"),
    heldOf("failed", undefined, "edit", "Edit"),
    heldOf("failed", RULED, "execute", "Bash"),
  ]),
  // a code mode script that failed and was tried another way: one quiet line inside the group (stage B's B4)
  {
    id: "UC-1246",
    words: stepWords("A script didn't run; Claude tried another way."),
    screen: "room/thread",
    render: () => (
      <RoomClicking
        host={() => threadHost({ entries: [QUESTION, ...codeModeAnswer(), ANSWER, step(91)] })}
        clicks={['[data-testid="step-group"] button']}
      />
    ),
  },
];

/** B4's notice for a code mode script that failed: kind "script_failed", level info, the host's English fallback. */
const SCRIPT_FAILED = {
  kind: "script_failed",
  level: "info",
  text: "A script didn't run; Claude tried another way.",
} as const;

/** One answer in code mode: two papers read, a script that failed, a paper and a search, another failure, then
 * Indian Kanoon. */
function codeModeAnswer(): ViewEntry[] {
  const at = (c: ToolCallEntry, order: number): ViewEntry => ({ tool_call: { ...c, order } });
  return [
    at(readOf(C, "Plaint.docx"), 40),
    at(readOf(C, "Written statement.pdf"), 41),
    noticeOf(42, SCRIPT_FAILED),
    at(readOf(C, "Loan agreement, 12 March 2019.pdf"), 43),
    at(searchOf(C, "notice period"), 44),
    noticeOf(45, SCRIPT_FAILED),
    at(mcpOf(C, "indian-kanoon", "search_judgments", KANOON), 46),
  ];
}

export default UC_STEP_LINES;
