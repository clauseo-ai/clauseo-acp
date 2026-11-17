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

/** Whether a model choice takes an effort (its `takes_effort`); a missing flag means it does. */
export const takesEffort = (choice: ConfigChoice | undefined): boolean =>
  choice?.takes_effort !== false;

/** The options the pickers show, model first: the first select option of each picked category
 * that has a value to choose. None for no options, and none for an option with no values, so no
 * menu is ever empty. The effort is left out while the model shown takes none. `modelShown`
 * gives the value the model's menu shows: the agent's current value by default; a draft's pick,
 * else the remembered, else that. */
export function pickables(
  options: readonly ConfigOption[] | undefined,
  modelShown: (model: Pickable) => string = (model) => model.current,
): readonly Pickable[] {
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
  const model = out.find((p) => p.category === "model");
  const takes =
    model === undefined || takesEffort(model.choices.find((c) => c.value === modelShown(model)));
  const kept = takes ? out : out.filter((p) => p.category !== "thought_level");
  return kept.length === 0 ? NONE : kept;
}

/** The value of an agent's "Default" row: the agent's own choice, whatever it stands at. */
export const DEFAULT_VALUE = "default";

/** The value of a choice the host makes up for an agent that hasn't reported its options yet: the one choice of each
 * option in its seed (AgentSummary.config_options before the agent's first session), meaning the agent picks its own
 * default. Mirrors the host's sentinel (crates/host/src/actor/agent_options/seed.rs, `AGENT_DEFAULT`); the host never
 * sends it to the agent and refuses a SetConfigOption of it, so a draft never sends it either. */
export const AGENT_DEFAULT_VALUE = "clauseo-agent-default";

/** The options a draft's pickers show: a seeded option is left out when the matter remembers a pick for it, since that
 * pick runs, not the agent's default the seed would name; with the agent's options lost, the draft shows no picker for
 * it, as before the seed. */
export function draftPickables(
  shown: readonly Pickable[],
  remembered: Readonly<Record<string, string>> | undefined,
): readonly Pickable[] {
  if (remembered === undefined) return shown;
  const kept = shown.filter(
    (p) => p.current !== AGENT_DEFAULT_VALUE || remembered[p.id] === undefined,
  );
  return kept.length === shown.length ? shown : kept;
}

/** What an option's "Default" row stands for, when the agent says: claude-agent-acp gives its model's Default row the
 * resolved model's name as the description ("Opus 5.5", session-model.js), and its effort's Default row none, since
 * Claude Code settles the effort itself and says nothing of it. Undefined with no Default row (codex-acp's values are
 * all concrete) or no description. */
export function resolvedDefault(p: Pickable): string | undefined {
  const row = p.choices.find((c) => c.value === DEFAULT_VALUE);
  const name = row?.description?.trim();
  return name === undefined || name === "" ? undefined : name;
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
 * options offer, and only the pickers' options; never the host's seed sentinel (AGENT_DEFAULT_VALUE), which the agent
 * settles by itself. */
export function draftConfig(
  options: readonly ConfigOption[] | undefined,
  picks: ConfigPicks | undefined,
  remembered: ConfigPicks | undefined,
): ConfigPicks | undefined {
  const shown = draftShown(options, picks, remembered);
  const picked = (p: Pickable) => offers(p, picks?.[p.id]) && picks?.[p.id] !== AGENT_DEFAULT_VALUE;
  if (picks === undefined || !shown.some(picked)) return undefined;
  const config: ConfigPicks = {};
  for (const p of shown) {
    const value = draftValue(p, picks[p.id], remembered?.[p.id]);
    if (offers(p, value) && value !== AGENT_DEFAULT_VALUE) config[p.id] = value;
  }
  return config;
}

/** The pickers a draft shows, over its picks and the matter's remembered ones: the effort is
 * left out while the draft's model takes none, so a pick or a remembered effort for it is never
 * sent. */
export const draftShown = (
  options: readonly ConfigOption[] | undefined,
  picks: ConfigPicks | undefined,
  remembered: ConfigPicks | undefined,
): readonly Pickable[] =>
  pickables(options, (m) => draftValue(m, picks?.[m.id], remembered?.[m.id]));

/** The command a pick in a conversation sends: the host checks the option's category, sets it, and keeps it in the
 * matter's remembered picks. */
export const setOption = (thread: ThreadId, option_id: string, value: string) => ({
  set_config_option: { thread, option_id, value: { select: value } },
});
