// The copy review entries for a step Claude's own checks refused before it ran (UC-600 to UC-612, #plain-refusal):
// the plain line under the row, in place of Claude Code's raw refusal, one entry for each line refusals.ts can give.
// Each is the real room over FakeTransport, its step group opened as the lawyer would, the refused read between two
// done steps; the raw text is Claude Code 0.3.293's, with a rule name that maps to the entry's line.
import type { ToolCallEntry, ToolCallStatus, ToolKind, ViewEntry } from "../transport/protocol.ts";
import { agentText, step, toolCall, userMessage } from "../testing/stream-script.ts";
import type { RefusalKey } from "@clauseo/components/lib/words-refusals.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomClicking } from "./states.tsx";
import { refusalLineWords } from "./words.ts";

const QUESTION = userMessage(5, 1, "What does the lease say about notice to the bank?");
const ANSWER = agentText(90, 1, "Clause nine needs thirty days' written notice to the bank.");
const SHARMA = "/Users/x/Matters/Sharma";

/** A call of `kind` on `file`, with `sent` as Claude Code's answer when there is one. */
function call(
  n: number,
  kind: ToolKind,
  status: ToolCallStatus,
  file: string,
  sent?: string,
): ViewEntry {
  const entry: ToolCallEntry = {
    ...toolCall(10 + n, 600 + n, status).tool_call!,
    kind,
    title: "",
    raw_input: { inline: JSON.stringify({ file_path: `${SHARMA}/${file}` }) },
    ...(sent === undefined
      ? {}
      : {
          raw_output: { inline: JSON.stringify(sent) },
          content: [{ text: { text: { inline: "```\n" + sent + "\n```" } } }],
        }),
  };
  return { tool_call: { ...entry, order: 10 + n } };
}

/** The safety check's block, naming `rule`. */
const blocked = (rule: string) =>
  `Permission for this action was denied by the Claude Code auto mode classifier. Reason: [${rule}] The action ` +
  "goes beyond the task. If you have other tasks that don't depend on this action, continue working on those.";
const NO_VERDICT =
  "The API told auto mode's safety classifier (claude-sonnet) to wait 2 minutes from now, so auto mode cannot " +
  "determine the safety of Read right now. This is not a judgment that the action is unsafe.";
const DENIED =
  "Permission to read /Users/x/Library/Application Support/Clauseo/agents/state.json has been denied.";

/** The settled thread: a paper read, the refused read, a search; its group opened. */
const refused = (sent: string) => () => (
  <RoomClicking
    host={() =>
      threadHost({
        entries: [
          QUESTION,
          call(1, "read", "completed", "Complaint.pdf"),
          call(2, "read", "failed", "Lease deed.pdf", sent),
          call(3, "read", "completed", "Annexure P-3.pdf"),
          ANSWER,
          step(91),
        ],
      })
    }
    clicks={['[data-testid="step-group"] button']}
  />
);

const M = "Nothing was changed.";
const S = "Claude's built-in safety check stopped this";
/** Each line, the raw text that brings it. */
const LINES: readonly (readonly [RefusalKey, string])[] = [
  [
    `${S}: it looked like reaching files or systems Claude wasn't asked to use. ${M}`,
    blocked("Unauthorized Access"),
  ],
  [
    `${S}: it looked like sending matter information somewhere outside. ${M}`,
    blocked("Data Exfiltration"),
  ],
  [
    `${S}: it looked like a search for passwords or sign-in keys. ${M}`,
    blocked("Credential Exploration"),
  ],
  [
    `${S}: it looked like deleting or overwriting work in a way that can't be undone. ${M}`,
    blocked("Irreversible Local Destruction"),
  ],
  [
    `${S}: it looked like getting around the checks on Claude's work. ${M}`,
    blocked("Undermining Oversight"),
  ],
  [`${S}: it looked like changing how Claude itself is set up. ${M}`, blocked("Self-Modification")],
  [
    `${S}: it looked like changing something outside this matter. ${M}`,
    blocked("Production Deploy"),
  ],
  [
    `${S}: it looked like following instructions hidden in a document or web page. ${M}`,
    blocked("Prompt Injection"),
  ],
  [
    `${S}: it looked like running code from a source that isn't trusted. ${M}`,
    blocked("Untrusted Code Execution"),
  ],
  [
    `${S}: it looked like spending money or acting for you outside Clauseo. ${M}`,
    blocked("Financial Transaction"),
  ],
  [`${S}. ${M}`, blocked("Something New")],
  [
    "Claude's built-in safety check couldn't look at this just now, so it didn't run. Nothing was changed.",
    NO_VERDICT,
  ],
  [
    "This tried to reach somewhere Clauseo keeps off limits here, such as Clauseo's own app data or another matter. Nothing was changed.",
    DENIED,
  ],
];

export const PLAIN_REFUSAL: Entry[] = LINES.map(([key, sent], i) => ({
  id: `UC-${600 + i}`,
  words: refusalLineWords(key),
  screen: "room/thread",
  render: refused(sent),
}));
