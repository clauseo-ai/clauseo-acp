// The model switch (host #1349's fix, StepEntry.model, effort and agent, copied from the turn's TurnStarted): a quiet
// divider above a question whose answer ran on another model or effort than the answer before it, when both came
// from the same agent. A change of agent is the handoff's divider; a step without the fields (an old log, a turn
// that never started) draws none. Only what changed is named: "Opus 5.5 → Sonnet 5.5", "High → Medium", or both.
// The divider shows once the question's answer has ended, since its step is where the fields are. Pure apart from
// the hook.
import { useStreamPart, useViewStore } from "../../store/hooks.ts";
import type { StreamView } from "../../store/view.ts";
import type {
  EntryId,
  NamedChoice,
  StepEntry,
  StreamId,
  TurnId,
} from "../../transport/protocol.ts";

/** The two runs of words a switch shows. */
export type ModelSwitch = { readonly from: string; readonly to: string };

/** What changed between two answers of the same agent, by name; null where nothing comparable changed. */
export function switchOf(before: StepEntry, after: StepEntry): ModelSwitch | null {
  if (before.agent === undefined || after.agent === undefined || before.agent !== after.agent)
    return null;
  const changed = (a?: NamedChoice, b?: NamedChoice) =>
    a !== undefined && b !== undefined && a.id !== b.id;
  const model = changed(before.model, after.model);
  const effort = changed(before.effort, after.effort);
  if (!model && !effort) return null;
  const run = (s: StepEntry) =>
    [model ? s.model?.name : undefined, effort ? s.effort?.name : undefined]
      .filter((n): n is string => n !== undefined && n.trim() !== "")
      .join(" ");
  const words = { from: run(before), to: run(after) };
  return words.from === "" || words.to === "" ? null : words;
}

const STEP = "step:";

/** Each turn's switch from the answer before it, one map per id list (a step joins the ids as its turn ends). */
const switchesOf = new WeakMap<readonly EntryId[], ReadonlyMap<TurnId, ModelSwitch>>();

function switches(v: StreamView): ReadonlyMap<TurnId, ModelSwitch> {
  let map = switchesOf.get(v.ids);
  if (map === undefined) {
    const built = new Map<TurnId, ModelSwitch>();
    let before: StepEntry | undefined;
    for (const id of v.ids) {
      if (!id.startsWith(STEP)) continue;
      const step = v.entries.get(id)?.step;
      if (step === undefined) continue;
      const change = before === undefined ? null : switchOf(before, step);
      if (change !== null) built.set(step.turn, change);
      before = step;
    }
    map = built;
    switchesOf.set(v.ids, map);
  }
  return map;
}

/** The switch above the question of `turn`, or undefined. */
export const switchFor = (v: StreamView | undefined, turn: TurnId): ModelSwitch | undefined =>
  v === undefined ? undefined : switches(v).get(turn);

export function useModelSwitch(stream: StreamId, turn: TurnId): ModelSwitch | undefined {
  return useStreamPart(useViewStore(), stream, (v) => switchFor(v, turn));
}
