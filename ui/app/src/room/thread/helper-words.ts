// A step that asks a helper, as the lawyer reads it (UC-906, UC-907). Two agents, two ways to name it:
// - Claude's Agent tool (its older name Task): claude-agent-acp sends the call's tool name, and its title is the
//   helper's description.
// - ChatGPT's spawn: codex-acp 2.2.2 titles it exactly `spawnAgent`, kind other, with no tool name; its task is the
//   spawn's own input, `raw_input.prompt`, exactly what ChatGPT told the helper (as helper-agents.ts on the
//   helper-cards branch reads it).
// Without this, Claude's call reads as "Thought it through." and counts as "Planned the work.". A step with no task
// keeps today's words (steps.ts). Pure.
import type { ToolCallEntry } from "../../transport/protocol.ts";
import { titleOf } from "./step-names.ts";
import { toolNameOf } from "./tool-names.ts";

/** Claude's helper tools, by tool name folded to lower case. */
const CLAUDE: ReadonlySet<string> = new Set(["agent", "task"]);
/** The title codex-acp 2.2.2 gives ChatGPT's spawn, with kind other and no tool name. */
const CHATGPT = "spawnAgent";
/** The most characters of a helper's task, the last one "…" when it is cut. */
const SHORT = 80;
/** Controls (Unicode Cc) and the bidi marks that could reorder a line around a task. */
const HIDDEN = /[\p{Cc}\u200E\u200F\u202A-\u202E\u2066-\u2069]/gu;

/** Whether a step asks a helper: Claude's by its tool name, ChatGPT's by its title and kind. */
export const helperOf = (entry: ToolCallEntry): boolean =>
  (entry.tool_name !== undefined && CLAUDE.has(entry.tool_name.toLowerCase())) ||
  (entry.title === CHATGPT && entry.kind === "other");

/** The spawn step's `raw_input.prompt`, from its input when the host sends it inline as JSON; null without one. */
function promptOf(entry: ToolCallEntry): string | null {
  const raw = entry.raw_input?.inline;
  if (raw === undefined) return null;
  try {
    const input: unknown = JSON.parse(raw);
    if (typeof input !== "object" || input === null) return null;
    const prompt = (input as Record<string, unknown>)["prompt"];
    return typeof prompt === "string" ? prompt : null;
  } catch {
    return null;
  }
}

/** A helper step's task as the lawyer reads it: Claude's title or ChatGPT's prompt, as one line (whitespace one space,
 * controls and bidi marks gone), its paths cut to their names, the marks and the full stop at its end off, cut to 80
 * characters with "…". "" when there is none, or the title is only a tool's name. */
export function helperTaskOf(entry: ToolCallEntry): string {
  const said =
    entry.title === CHATGPT
      ? promptOf(entry)
      : toolNameOf(entry.title) === undefined
        ? entry.title
        : null;
  if (said === null) return "";
  const line = said.replace(/\s+/g, " ").replace(HIDDEN, "").trim();
  const task = titleOf(line)
    .replace(/[.!?।…]+$/, "")
    .trim();
  const chars = Array.from(task);
  return chars.length > SHORT
    ? `${chars
        .slice(0, SHORT - 1)
        .join("")
        .trimEnd()}…`
    : task;
}
