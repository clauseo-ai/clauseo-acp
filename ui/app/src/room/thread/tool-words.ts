// A step titled with a tool's name (tool-names.ts), once it is over: the known name's words, done or tried, else the
// kind's own plain line. For the rows and notes that tell a step after it ran: the settled group (steps.ts), a lone
// step's row and a steer's note.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { steps, type StepKey } from "@clauseo/components/lib/words-steps.ts";
import type { ToolCallEntry } from "../../transport/protocol.ts";
import { guideOf, skillOf, skillTitle } from "./skill-words.ts";
import { toolNameOf, type ToolAs } from "./tool-names.ts";

const DONE: Record<ToolAs, StepKey> = {
  network: "Let a task reach the internet.",
  execute: "Ran a task.",
  read: "Read the papers.",
  edit: "Changed a paper.",
  search: "Searched the papers.",
  fetch: "Looked online.",
};
const TRIED: Record<ToolAs, StepKey> = {
  network: "Tried to let a task reach the internet.",
  execute: "Tried to run a task.",
  read: "Tried to read the papers.",
  edit: "Tried to change a paper.",
  search: "Tried to search the papers.",
  fetch: "Tried to look online.",
};

/** A known name's words once its step is over: done, or tried (failed or stopped). */
export const toolDone = (lang: Lang, as: ToolAs, tried: boolean): string =>
  steps(lang, tried ? TRIED[as] : DONE[as]);

/** A skill's row once over: "Read Clauseo's drafting guide.", or "Read a guide." for one Clauseo doesn't ship. */
function guideDone(lang: Lang, skill: { name: string }, tried: boolean): string {
  const guide = guideOf(lang, skill);
  if (guide === null) return steps(lang, tried ? "Tried to read a guide." : "Read a guide.");
  return steps(lang, tried ? "Tried to read {name}." : "Read {name}.", { name: guide });
}

/** A skill step's row once over, or null when the call loads no skill (skill-words.ts). */
export function skillDone(lang: Lang, entry: ToolCallEntry, tried: boolean): string | null {
  const skill = skillOf(entry);
  return skill === null ? null : guideDone(lang, skill, tried);
}

/** A title as a row shows it once its step is over: the title itself when it is words; for a tool's name, the known
 * name's words, else "Did one other thing." (or "Tried one other thing."). */
export function overTitle(lang: Lang, title: string, tried: boolean): string {
  const skill = skillTitle(title);
  if (skill !== null) return guideDone(lang, skill, tried);
  const tool = toolNameOf(title);
  if (tool === undefined) return title;
  if (tool.as !== null) return toolDone(lang, tool.as, tried);
  return steps(lang, tried ? "Tried one other thing." : "Did one other thing.");
}
