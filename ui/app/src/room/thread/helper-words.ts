// A step that asks a helper, as the lawyer reads it (UC-906, UC-907): Claude's Agent tool (its older name Task) and
// ChatGPT's spawn_agent, named by the call's tool name (`tool_name`, as skill-words.ts names a skill). claude-agent-acp
// reports the call with the kind think and the helper's description as its title, so without this it reads as "Thought
// it through." and counts as "Planned the work.". The title is the helper's task, cut to 80 characters; a title that is
// only a tool's name (codex-acp titles ChatGPT's spawnAgent that way) is no task, and the step keeps its old words.
import type { ToolCallEntry } from "../../transport/protocol.ts";
import { titleOf } from "./step-names.ts";
import { toolNameOf } from "./tool-names.ts";

/** The tool names a helper's call carries, folded to lower case: Claude's Agent and Task, ChatGPT's spawn_agent. */
const HELPERS: ReadonlySet<string> = new Set(["agent", "task", "spawn_agent"]);
/** The most characters of a helper's task, the last one "…" when it is cut. */
const SHORT = 80;

/** Whether a step asks a helper: its tool name is one of HELPERS. */
export const helperOf = (entry: ToolCallEntry): boolean =>
  entry.tool_name !== undefined && HELPERS.has(entry.tool_name.toLowerCase());

/** A helper step's task as the lawyer reads it: its title with the paths cut and the marks off, without the full stop
 * at its end, cut to 80 characters; "" when the title is empty or only a tool's name. */
export function helperTaskOf(entry: ToolCallEntry): string {
  if (toolNameOf(entry.title) !== undefined) return "";
  const task = titleOf(entry.title)
    .replace(/[.!?।…]+$/, "")
    .trim();
  return task.length > SHORT ? `${task.slice(0, SHORT - 1)}…` : task;
}
