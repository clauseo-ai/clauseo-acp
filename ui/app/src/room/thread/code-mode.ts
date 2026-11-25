// Whether the agent runs in code mode, said once, as a quiet line at the foot of the composer's model menu ("Code mode
// on"), where the lawyer looks for the model and effort. Never a mark on an answer: on today's plans nearly every
// answer runs in code mode, so a mark there would only repeat. The truth, by agent:
// - ChatGPT: always (Codex runs every plan model in code mode).
// - Claude: as its conversation's last finished answer ran (StepEntry.code_mode, from its TurnEnded, so a reload keeps
//   it); before any answer has finished, when the model picked is Claude 5.5 or newer (Claude Code's own rule, seen
//   live on Haiku, Sonnet and Opus 5.5). A finished answer that didn't run in code mode leaves the line out, with no
//   warning.
// - Any other agent: no line.
import type { AgentKind, ModelEntry } from "../../transport/protocol.ts";
import type { StreamView } from "../../store/view.ts";

/** The first Claude version that runs in code mode. */
const FIRST: readonly [number, number] = [5, 5];

/** A model's version, from its name ("Opus 5.5", "Sonnet 6"), else its description ("Opus 5.5 · Most capable"), else
 * its id ("claude-opus-4-5-20251101", "opus-5-5"): the first number of one or two digits, with the one or two after a
 * dot or a dash as its minor. A date, a longer number or one run into letters ("1M") is no version. Null when none
 * says one ("default", "Default (recommended)"). */
export function modelVersion(model: {
  id: string;
  name: string;
  description?: string | undefined;
}): [number, number] | null {
  for (const text of [model.name, model.description ?? "", model.id]) {
    const found = /(?:^|[^\d.])(\d{1,2})(?:[.-](\d{1,2}))?(?![\d.a-zA-Z])/.exec(text);
    if (found !== null) return [Number(found[1]), Number(found[2] ?? 0)];
  }
  return null;
}

/** Whether a Claude model runs in code mode by its version alone: 5.5 or newer. One with no version, no. */
export function claudeCodeModeModel(
  model: Pick<ModelEntry, "id" | "name" | "description">,
): boolean {
  const v = modelVersion(model);
  return v !== null && (v[0] > FIRST[0] || (v[0] === FIRST[0] && v[1] >= FIRST[1]));
}

/** The conversation's last finished answer: whether it ran in code mode, undefined before any has finished. A step is
 * written only as a turn ends, so the last step in the view is the last finished answer's. */
export const lastCodeMode = (v: StreamView | undefined): boolean | undefined => {
  if (v === undefined) return undefined;
  for (let i = v.ids.length - 1; i >= 0; i -= 1) {
    const step = v.entries.get(v.ids[i]!)?.step;
    if (step !== undefined) return step.code_mode === true;
  }
  return undefined;
};

/** Whether the model menu says "Code mode on": by the agent's kind, its conversation's last finished answer
 * (undefined for none, as for a new conversation) and the model picked. */
export function codeModeLine(
  kind: AgentKind | undefined,
  last: boolean | undefined,
  model: Pick<ModelEntry, "id" | "name" | "description"> | undefined,
): boolean {
  if (kind === "codex") return true;
  if (kind !== "claude") return false;
  if (last !== undefined) return last;
  return model !== undefined && claudeCodeModeModel(model);
}
