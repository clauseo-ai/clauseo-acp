// The model and effort pickers' rules, pure (the host's half: crates/host/src/actor/agent_options/models.rs and
// crates/host/src/actor/turn/prompt_picks.rs). The menus read an agent's models by name (AgentSummary.models): every
// model the host sends, in its order, each with its own efforts, so the effort menu follows the model shown. A new
// conversation's draft shows the lawyer's pick, else the matter's remembered pick (MatterSummary.agent_prefs) where
// the agent still offers it, else the model and effort a new session starts on; its picks go with create_thread under
// the agent's own keys (AgentModels.model_option, effort_option). A conversation reads its header's options as the
// host's `view` reads them, with the agent's models for each model's efforts; its line names the turn's record, and a
// pick there stays in the composer until a send carries it as SendPrompt.config.
// Only the model and the effort are ever drawn: any other option (a mode, Claude's fast mode, ChatGPT's
// collaboration mode) is not read at all. Until the host's probe has filled an agent's models there is nothing to
// draw, and the status line says the list is loading (config-menus.ts); nothing here makes up a default.
import type {
  AgentModels,
  ConfigChoice,
  ConfigOption,
  ConfigPicks,
  ModelEntry,
  NamedChoice,
} from "../transport/protocol.ts";

/** The options a conversation's header is read for, as the host reads them: the model, then the effort. */
export const PICKED_CATEGORIES = ["model", "thought_level"] as const;

const NO_EFFORTS: readonly NamedChoice[] = [];
const NO_CHOICES: readonly ConfigChoice[] = [];

/** The choices of a conversation's model menu, from its header's options: the first select option of the model
 * category that has any (as the host's `view` reads it), none without one. A helper's card names its model by them. */
export function modelChoices(
  options: readonly ConfigOption[] | undefined,
): readonly ConfigChoice[] {
  for (const o of options ?? []) {
    if (o.category !== "model" || typeof o.kind !== "object" || o.kind.select === undefined)
      continue;
    if (o.kind.select.options.length > 0) return o.kind.select.options;
  }
  return NO_CHOICES;
}

/** What the two menus show: the models, the model shown, that model's efforts, and the effort shown ("" for none:
 * a model that takes none, or one whose efforts give no value to stand on). */
export type Shown = {
  models: AgentModels;
  model: string;
  efforts: readonly NamedChoice[];
  effort: string;
  /** A conversation's record: "" means the turn named none, and its menu is left out, never filled with a guess. */
  record?: true;
};

/** The efforts a model takes, lowest first; none for a model the list doesn't hold or the agent with no effort. */
export function effortsOf(models: AgentModels, model: string): readonly NamedChoice[] {
  if (models.effort_option === undefined) return NO_EFFORTS;
  return models.models.find((m) => m.id === model)?.efforts ?? NO_EFFORTS;
}

/** The effort a model runs with: the first of `wanted` it takes, else its default effort; "" with neither. So a change
 * of model keeps the effort when the new model takes it, else takes that model's default. */
export function effortFor(
  models: AgentModels,
  model: string,
  ...wanted: (string | undefined)[]
): string {
  const efforts = effortsOf(models, model);
  const takes = (id: string | undefined): id is string =>
    id !== undefined && efforts.some((e) => e.id === id);
  const kept = wanted.find(takes);
  if (kept !== undefined) return kept;
  const fallback = models.models.find((m) => m.id === model)?.default_effort;
  return takes(fallback) ? fallback : "";
}

/** Whether the list holds the model. */
const offers = (models: AgentModels, model: string | undefined): model is string =>
  model !== undefined && models.models.some((m) => m.id === model);

/** What a new conversation's menus show: the draft's pick, else the matter's remembered one, each only while the
 * agent offers it, else what a new session starts on. Undefined until the agent's models are in. */
export function draftShown(
  models: AgentModels | undefined,
  picks: ConfigPicks | undefined,
  remembered: ConfigPicks | undefined,
): Shown | undefined {
  if (models === undefined || models.models.length === 0) return undefined;
  const key = models.model_option;
  const model =
    [picks?.[key], remembered?.[key]].find((m) => offers(models, m)) ?? models.current_model;
  const ek = models.effort_option;
  const starts = model === models.current_model ? models.current_effort : undefined;
  const effort =
    ek === undefined ? "" : effortFor(models, model, picks?.[ek], remembered?.[ek], starts);
  return { models, model, efforts: effortsOf(models, model), effort };
}

/** What `create_thread` (or a held agent's `send_prompt`) sends as `config`: nothing while the lawyer picked nothing,
 * so the host applies the matter's remembered picks or the agent's own by itself; otherwise the model and the effort
 * as the menus show them, under the agent's keys, so a pick of the model alone keeps the effort shown with it. */
export function draftConfig(
  models: AgentModels | undefined,
  picks: ConfigPicks | undefined,
  remembered: ConfigPicks | undefined,
): ConfigPicks | undefined {
  const shown = draftShown(models, picks, remembered);
  if (shown === undefined || picks === undefined) return undefined;
  const ek = shown.models.effort_option;
  const picked =
    picks[shown.models.model_option] !== undefined || (ek !== undefined && picks[ek] !== undefined);
  if (!picked) return undefined;
  const config: ConfigPicks = { [shown.models.model_option]: shown.model };
  if (ek !== undefined && shown.effort !== "") config[ek] = shown.effort;
  return config;
}

/** The picks a change of model makes in a draft or a hold: the model, and the effort the new model takes (the one
 * shown when it takes it, else its default), so the effort menu never stands on a value the model doesn't have. */
export function modelPick(shown: Shown, model: string): ConfigPicks {
  const picks: ConfigPicks = { [shown.models.model_option]: model };
  const ek = shown.models.effort_option;
  const effort = effortFor(shown.models, model, shown.effort);
  if (ek !== undefined && effort !== "") picks[ek] = effort;
  return picks;
}

/** The first select option of a category, as the host's `select_of` finds it. */
function selectOf(options: readonly ConfigOption[], category: (typeof PICKED_CATEGORIES)[number]) {
  for (const o of options) {
    if (o.category !== category || typeof o.kind !== "object" || o.kind.select === undefined)
      continue;
    return { id: o.id, current: o.kind.select.current_value, choices: o.kind.select.options };
  }
  return undefined;
}

const named = (c: ConfigChoice): NamedChoice => ({ id: c.value, name: c.name });

/** A conversation's models, as the host's `view` makes an agent's from its kept options: the model option's choices
 * (named and described as the session gives them), each with its efforts from the agent's models, the current model
 * taking the effort option's levels when the agent's list doesn't hold it; without a model option, the agent's
 * models as they are. Every option but the model and the effort is left unread. */
export function threadModels(
  options: readonly ConfigOption[],
  probed: AgentModels | undefined,
): AgentModels | undefined {
  const model = selectOf(options, "model");
  if (model === undefined || model.choices.length === 0) return probed;
  const effort = selectOf(options, "thought_level");
  const levels = effort?.choices.map(named) ?? [];
  const models = model.choices.map((c): ModelEntry => {
    const known = probed?.models.find((m) => m.id === c.value);
    const efforts = known?.efforts ?? (c.value === model.current ? levels : []);
    const description = (c.description ?? known?.description)?.trim();
    return {
      id: c.value,
      name: c.name,
      ...(description === undefined || description === "" ? {} : { description }),
      efforts,
      ...(known?.default_effort === undefined ? {} : { default_effort: known.default_effort }),
    };
  });
  const effortOption = effort?.id ?? probed?.effort_option;
  return {
    model_option: model.id,
    ...(effortOption === undefined ? {} : { effort_option: effortOption }),
    models,
    current_model: model.current,
    ...(effort === undefined ? {} : { current_effort: effort.current }),
  };
}

/** What a conversation's line shows: its record, what the running (else the latest) turn runs with (the header's
 * `model` and `effort`, set by each TurnStarted, which clears what it leaves out); before any record, the session's
 * options. A value the record leaves out shows nothing, never a guess, and while a turn runs only the record shows. A
 * model the turn ran with that the list no longer holds is listed by its turn's name, so the trigger never shows an
 * id. The lawyer's pick for the next message is apart from it (nextPicks). */
export function threadShown(
  options: readonly ConfigOption[],
  probed: AgentModels | undefined,
  ran: {
    model?: NamedChoice | undefined;
    effort?: NamedChoice | undefined;
    running?: boolean | undefined;
  },
): Shown | undefined {
  const base = threadModels(options, probed);
  if (base === undefined || base.models.length === 0) return undefined;
  const ranModel = ran.model;
  if (ranModel === undefined) {
    if (ran.running === true || selectOf(options, "model") === undefined)
      return { models: base, model: "", efforts: [], effort: "", record: true };
    const model = base.current_model;
    const effort = effortFor(base, model, base.current_effort);
    return { models: base, model, efforts: effortsOf(base, model), effort };
  }
  const models = offers(base, ranModel.id)
    ? base
    : { ...base, models: [{ id: ranModel.id, name: ranModel.name }, ...base.models] };
  const listed = effortsOf(models, ranModel.id);
  // an effort the list doesn't hold for the model is still named as the turn ran with it
  const ranEffort = ran.effort;
  const efforts =
    ranEffort === undefined || listed.some((e) => e.id === ranEffort.id)
      ? listed
      : [...listed, ranEffort];
  return {
    models,
    model: ranModel.id,
    efforts: ranEffort === undefined ? [] : efforts,
    effort: ranEffort?.id ?? "",
    record: true,
  };
}

/** The lawyer's pick for the next message in a conversation: kept in the composer until a send carries it. */
export type NextPick = { model?: string; effort?: string };

/** What the next message goes with: the pick over what the line shows, a picked model with the effort it takes (the
 * one shown when it takes it, else its default). "" for none. */
export function nextPicks(shown: Shown, pick: NextPick | null): { model: string; effort: string } {
  const model = pick?.model ?? shown.model;
  const effort =
    pick?.effort ??
    (pick?.model === undefined ? shown.effort : effortFor(shown.models, model, shown.effort));
  return { model, effort };
}

/** SendPrompt.config for the next message, under the agent's own keys; undefined with nothing to name. */
export function configOf(
  models: AgentModels,
  next: { model: string; effort: string },
): ConfigPicks | undefined {
  const config: ConfigPicks = {};
  if (next.model !== "") config[models.model_option] = next.model;
  if (models.effort_option !== undefined && next.effort !== "")
    config[models.effort_option] = next.effort;
  return Object.keys(config).length === 0 ? undefined : config;
}

/** Whether a pick still stands: until a turn starts whose record matches it (the record then names it), or a
 * pick_not_used notice comes after it (the agent couldn't use it, and the line shows what ran). A turn ending, or a
 * steer joining the running answer, leaves it. */
export function pickStands(
  pick: NextPick & { notices: number },
  ran: { model?: NamedChoice | undefined; effort?: NamedChoice | undefined },
  notices: number,
): boolean {
  if (notices > pick.notices) return false;
  const model = pick.model === undefined || ran.model?.id === pick.model;
  const effort = pick.effort === undefined || ran.effort?.id === pick.effort;
  return !(model && effort);
}
