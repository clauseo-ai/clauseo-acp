// A step titled with a tool's name (tool-names.ts), once it is over: the known name's words, done or tried, else the
// tool's readable name ("Used Indian Kanoon: search judgments."). For the rows and notes that tell a step after it
// ran: the settled group (steps.ts), a lone step's row and a steer's note.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { steps, type StepKey } from "@clauseo/components/lib/words-steps.ts";
import type { ToolCallEntry } from "../../transport/protocol.ts";
import { skillOf, skillTitle } from "./skill-words.ts";
import {
  caseLawOf,
  readableTool,
  toolLabelOf,
  toolNameOf,
  type CaseLaw,
  type ToolAs,
} from "./tool-names.ts";

const DONE: Record<ToolAs, StepKey> = {
  network: "Let a task reach the internet.",
  execute: "Ran a command.",
  read: "Read the papers.",
  edit: "Changed a paper.",
  search: "Searched the papers.",
  fetch: "Looked online.",
};
const TRIED: Record<ToolAs, StepKey> = {
  network: "Tried to let a task reach the internet.",
  execute: "Tried to run a command.",
  read: "Tried to read the papers.",
  edit: "Tried to change a paper.",
  search: "Tried to search the papers.",
  fetch: "Tried to look online.",
};

/** A case-law tool's words (tool-names.ts, caseLawOf) once its step is over: done, and tried. */
const CASE_LAW_DONE: Record<CaseLaw, [done: StepKey, tried: StepKey]> = {
  search: ["Searched case law.", "Tried to search case law."],
  credits: ["Checked your case-law credits.", "Tried to check your case-law credits."],
  payment: [
    "Got a payment link for case-law credits.",
    "Tried to get a payment link for case-law credits.",
  ],
};

/** A step titled with a case-law tool's name, once over: its words, done or tried; null for any other title. */
export function caseLawDone(lang: Lang, title: string, tried: boolean): string | null {
  const law = caseLawOf(title);
  return law === null ? null : steps(lang, CASE_LAW_DONE[law][tried ? 1 : 0]);
}

/** A known name's words once its step is over: done, or tried (failed or stopped). */
export const toolDone = (lang: Lang, as: ToolAs, tried: boolean): string =>
  steps(lang, tried ? TRIED[as] : DONE[as]);

/** A skill's row once over, by the skill's name made readable: "Used the legal drafting skill.", or "Used a skill."
 * for one that names none (issue 13; until 24 Nov a guide's: "Read Clauseo's drafting guide."). */
function guideDone(lang: Lang, skill: { name: string }, tried: boolean): string {
  if (skill.name === "") return steps(lang, tried ? "Tried to use a skill." : "Used a skill.");
  return steps(lang, tried ? "Tried to use the {skill} skill." : "Used the {skill} skill.", {
    skill: readableTool(skill.name),
  });
}

/** A tool's row once over, by its readable name (tool-names.ts, toolLabelOf): "Used Indian Kanoon: search
 * judgments."; "Used a tool." with none. */
export function usedDone(lang: Lang, label: string | null, tried: boolean): string {
  if (label === null) return steps(lang, tried ? "Tried to use a tool." : "Used a tool.");
  return steps(lang, tried ? "Tried to use {tool}." : "Used {tool}.", { tool: label });
}

/** A skill step's row once over, or null when the call loads no skill (skill-words.ts). */
export function skillDone(lang: Lang, entry: ToolCallEntry, tried: boolean): string | null {
  const skill = skillOf(entry);
  return skill === null ? null : guideDone(lang, skill, tried);
}

/** A title as a row shows it once its step is over: the title itself when it is words; for a tool's name, the known
 * name's words, else the name made readable ("Used todo write.", or "Tried to use todo write."). */
export function overTitle(lang: Lang, title: string, tried: boolean): string {
  const skill = skillTitle(title);
  if (skill !== null) return guideDone(lang, skill, tried);
  const law = caseLawDone(lang, title, tried);
  if (law !== null) return law;
  const tool = toolNameOf(title);
  if (tool === undefined) return title;
  if (tool.as !== null) return toolDone(lang, tool.as, tried);
  return usedDone(lang, toolLabelOf({ title }), tried);
}
