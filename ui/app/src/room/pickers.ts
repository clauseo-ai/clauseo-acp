// The model and effort pickers' rules, pure (the host's half: crates/host/src/actor/turn/picks.rs). The
// model is the agent's select option of category `model`, the effort its select option of category `thought_level`;
// any other option (a mode, say) is never offered, since the host refuses it
// from the window. A new conversation's draft reads the agent's options as its last session reported them
// (AgentSummary.config_options); each value shown is the lawyer's pick in the draft, else the matter's remembered pick
// (MatterSummary.agent_prefs) where the agent still offers it, else the agent's own current value. A conversation reads
// its own options from its header, and a pick there is a SetConfigOption.
import type { ConfigChoice, ConfigOption, ConfigPicks, ThreadId } from "../transport/protocol.ts";

/** The pickers, in the order the status line shows them: the model, then the effort. */
export const PICKED_CATEGORIES = ["model", "thought_level"] as const;
export type PickedCategory = (typeof PICKED_CATEGORIES)[number];

/** A select option the window may set: its id, its category, its current value and its values. */
export type Pickable = {
  id: string;
  category: PickedCategory;
  current: string;
  choices: readonly ConfigChoice[];
};

const NONE: readonly Pickable[] = [];

/** The options the pickers show, model first: the first select option of each picked category that has a value to
 * choose. None for no options, and none for an option with no values, so no menu is ever empty. */
export function pickables(options: readonly ConfigOption[] | undefined): readonly Pickable[] {
  if (options === undefined || options.length === 0) return NONE;
  const out: Pickable[] = [];
  for (const category of PICKED_CATEGORIES) {
    for (const o of options) {
      if (o.category !== category || typeof o.kind !== "object" || o.kind.select === undefined)
        continue;
      const { current_value, options: choices } = o.kind.select;
      if (choices.length === 0) continue;
      out.push({ id: o.id, category, current: current_value, choices });
      break;
    }
  }
  return out.length === 0 ? NONE : out;
}

/** Whether the option still offers `value`. */
export const offers = (p: Pickable, value: string | undefined): value is string =>
  value !== undefined && p.choices.some((c) => c.value === value);

/** The value a draft shows for an option: the draft's pick, else the matter's remembered one, each only while the
 * option offers it, else the agent's current value. */
export function draftValue(p: Pickable, pick?: string, remembered?: string): string {
  if (offers(p, pick)) return pick;
  if (offers(p, remembered)) return remembered;
  return p.current;
}

/** What `create_thread` sends as `config`: nothing while the lawyer picked nothing in the draft, so the host applies
 * the matter's remembered picks or the agent's defaults by itself; otherwise every picker's value as the draft shows
 * it (the picks over the remembered ones), so a pick of the model alone keeps the remembered effort. Only values the
 * options offer, and only the pickers' options. */
export function draftConfig(
  options: readonly ConfigOption[] | undefined,
  picks: ConfigPicks | undefined,
  remembered: ConfigPicks | undefined,
): ConfigPicks | undefined {
  const shown = pickables(options);
  if (picks === undefined || !shown.some((p) => offers(p, picks[p.id]))) return undefined;
  const config: ConfigPicks = {};
  for (const p of shown) {
    const value = draftValue(p, picks[p.id], remembered?.[p.id]);
    if (offers(p, value)) config[p.id] = value;
  }
  return config;
}

/** The command a pick in a conversation sends: the host checks the option's category, sets it, and keeps it in the
 * matter's remembered picks. */
export const setOption = (thread: ThreadId, option_id: string, value: string) => ({
  set_config_option: { thread, option_id, value: { select: value } },
});
