// pickers.ts's rules: the models and efforts a draft or a conversation shows, the effort that follows a change of
// model, and what create_thread (or a held plan's send_prompt) sends as config, under each agent's own keys.
import { expect, test } from "vitest";
import type { AgentModels, ConfigOption } from "../transport/protocol.ts";
import { menusOf } from "./config-menus.ts";
import {
  draftConfig,
  draftShown,
  effortFor,
  modelPick,
  threadModels,
  threadShown,
  nextPicks,
  configOf,
  pickStands,
} from "./pickers.ts";

const level = (id: string) => ({ id, name: id[0]!.toUpperCase() + id.slice(1) });
const LMH = ["low", "medium", "high"].map(level);

/** Claude's, as the probe gives them: Opus takes three efforts, Fable two, Haiku none. */
const CLAUDE: AgentModels = {
  model_option: "model",
  effort_option: "effort",
  current_model: "opus",
  current_effort: "high",
  models: [
    {
      id: "opus",
      name: "Opus 5.5",
      description: "Most capable",
      efforts: LMH,
      default_effort: "medium",
    },
    {
      id: "fable",
      name: "Fable 5.2",
      efforts: [level("low"), level("medium")],
      default_effort: "medium",
    },
    { id: "haiku", name: "Haiku 5.5", efforts: [] },
  ],
};

/** ChatGPT's, its effort under its own key. */
const CODEX: AgentModels = {
  model_option: "model",
  effort_option: "reasoning_effort",
  current_model: "gpt-6.2-sol",
  current_effort: "medium",
  models: [
    { id: "gpt-6.2-sol", name: "gpt-6.2-sol", efforts: LMH, default_effort: "medium" },
    { id: "gpt-6-luna", name: "gpt-6-luna", efforts: LMH, default_effort: "low" },
  ],
};

const select = (id: string, category: string, current: string, values: string[]): ConfigOption =>
  ({
    id,
    name: id,
    category,
    kind: {
      select: { current_value: current, options: values.map((v) => ({ value: v, name: v })) },
    },
  }) as ConfigOption;

test("a draft shows its pick, else the matter's remembered value, else what a new session starts on", () => {
  expect(draftShown(CLAUDE, undefined, undefined)).toMatchObject({ model: "opus", effort: "high" });
  expect(draftShown(CLAUDE, undefined, { model: "fable", effort: "low" })).toMatchObject({
    model: "fable",
    effort: "low",
  });
  expect(draftShown(CLAUDE, { model: "opus" }, { model: "fable", effort: "low" })).toMatchObject({
    model: "opus",
    effort: "low",
  });
  // a remembered model the agent no longer offers: what a new session starts on
  expect(draftShown(CLAUDE, undefined, { model: "retired" })).toMatchObject({ model: "opus" });
  // no models yet: nothing to show, and nothing made up
  expect(draftShown(undefined, { model: "opus" }, undefined)).toBeUndefined();
  expect(draftShown({ ...CLAUDE, models: [] }, undefined, undefined)).toBeUndefined();
});

test("a change of model keeps the effort the new model takes, else takes that model's default", () => {
  expect(effortFor(CLAUDE, "fable", "low")).toBe("low");
  expect(effortFor(CLAUDE, "fable", "high")).toBe("medium");
  expect(effortFor(CLAUDE, "haiku", "high")).toBe("");
  const shown = draftShown(CLAUDE, undefined, undefined)!;
  expect(modelPick(shown, "fable")).toEqual({ model: "fable", effort: "medium" });
  expect(modelPick(shown, "haiku")).toEqual({ model: "haiku" });
  const low = draftShown(CLAUDE, { effort: "low" }, undefined)!;
  expect(modelPick(low, "fable")).toEqual({ model: "fable", effort: "low" });
  // ChatGPT's effort goes under its own key
  expect(modelPick(draftShown(CODEX, undefined, undefined)!, "gpt-6-luna")).toEqual({
    model: "gpt-6-luna",
    reasoning_effort: "medium",
  });
});

test("create_thread's config: none without a pick; a pick of the model alone keeps the effort shown", () => {
  expect(draftConfig(CLAUDE, undefined, { model: "fable" })).toBeUndefined();
  expect(draftConfig(CLAUDE, {}, undefined)).toBeUndefined();
  expect(draftConfig(CLAUDE, { model: "opus" }, { effort: "low" })).toEqual({
    model: "opus",
    effort: "low",
  });
  // a model that takes no effort sends none, even one remembered
  expect(draftConfig(CLAUDE, { model: "haiku" }, { effort: "low" })).toEqual({ model: "haiku" });
  expect(draftConfig(CODEX, { reasoning_effort: "high" }, undefined)).toEqual({
    model: "gpt-6.2-sol",
    reasoning_effort: "high",
  });
  expect(draftConfig(undefined, { model: "opus" }, undefined)).toBeUndefined();
});

test("every model the host sends is listed, in its order, a description only where the agent gives one", () => {
  const many: AgentModels = {
    ...CLAUDE,
    models: Array.from({ length: 12 }, (_, i) => ({
      id: `m${i}`,
      name: `Model ${i}`,
      efforts: LMH,
    })),
    current_model: "m11",
  };
  const [model, effort] = menusOf("en", draftShown(many, undefined, undefined)!, () => {});
  expect(model!.options.map((o) => o.name)).toEqual(many.models.map((m) => m.name));
  expect(model!.value).toBe("m11");
  expect(effort!.options.map((o) => o.name)).toEqual(["Low", "Medium", "High"]);
  const [claude] = menusOf("en", draftShown(CLAUDE, undefined, undefined)!, () => {});
  expect(claude!.options.map((o) => o.description)).toEqual(["Most capable", undefined, undefined]);
  // no seed words, no default note
  const said = JSON.stringify(menusOf("en", draftShown(CLAUDE, undefined, undefined)!, () => {}));
  for (const word of ["default", "Default"]) expect(said).not.toContain(word);
});

test("the effort menu follows the model shown, and is left out for a model that takes none", () => {
  const fable = menusOf("en", draftShown(CLAUDE, { model: "fable" }, undefined)!, () => {});
  expect(fable[1]!.options.map((o) => o.value)).toEqual(["low", "medium"]);
  expect(fable[1]!.value).toBe("medium");
  expect(menusOf("en", draftShown(CLAUDE, { model: "haiku" }, undefined)!, () => {})).toHaveLength(
    1,
  );
});

test("a conversation's models: its header's model option, each model's efforts from the agent's list", () => {
  const options = [
    select("mode", "mode", "default", ["default", "plan"]),
    select("model", "model", "fable", ["opus", "fable", "new-one"]),
    select("effort", "thought_level", "low", ["low", "medium"]),
    select("fast", "other", "off", ["off", "on"]),
  ];
  const models = threadModels(options, CLAUDE)!;
  expect(models.models.map((m) => m.id)).toEqual(["opus", "fable", "new-one"]);
  expect(models.models[0]!.efforts).toEqual(LMH);
  // a model the agent's list doesn't hold takes no efforts unless it is the current one
  expect(models.models[2]!.efforts).toEqual([]);
  expect(models).toMatchObject({
    current_model: "fable",
    current_effort: "low",
    effort_option: "effort",
  });
  // no model option in the header: the agent's own list
  expect(threadModels([], CLAUDE)).toBe(CLAUDE);
  expect(threadModels([options[0]!], undefined)).toBeUndefined();
});

test("a conversation's line names the record, during and after a turn; the options only before any record", () => {
  const options = [
    select("model", "model", "opus", ["opus", "fable", "haiku"]),
    select("effort", "thought_level", "high", ["low", "medium", "high"]),
  ];
  const ran = { model: { id: "fable", name: "Fable 5.2" }, effort: { id: "low", name: "Low" } };
  expect(threadShown(options, CLAUDE, ran)).toMatchObject({ model: "fable", effort: "low" });
  expect(threadShown(options, CLAUDE, { ...ran, running: true })).toMatchObject({
    model: "fable",
    effort: "low",
  });
  // before any record: the session's options; while a turn runs with none: nothing
  expect(threadShown(options, CLAUDE, {})).toMatchObject({ model: "opus", effort: "high" });
  expect(threadShown(options, CLAUDE, { running: true })).toMatchObject({ model: "", effort: "" });
  // a turn that names its model only: no effort, never the last one
  expect(threadShown(options, CLAUDE, { model: ran.model })).toMatchObject({
    model: "fable",
    effort: "",
  });
  // a model the list no longer holds, named as it ran
  const gone = threadShown([], CLAUDE, { model: { id: "old", name: "Opus 4" } })!;
  expect(gone.model).toBe("old");
  expect(gone.models.models[0]).toEqual({ id: "old", name: "Opus 4" });
  expect(threadShown([], undefined, ran)).toBeUndefined();
});

test("the next message's picks: the pick over the record, a picked model with the effort it takes", () => {
  const shown = threadShown([], CLAUDE, {
    model: { id: "opus", name: "Opus 5.5" },
    effort: { id: "high", name: "High" },
  })!;
  expect(nextPicks(shown, null)).toEqual({ model: "opus", effort: "high" });
  expect(nextPicks(shown, { effort: "low" })).toEqual({ model: "opus", effort: "low" });
  // Fable has no High: its default
  expect(nextPicks(shown, { model: "fable" })).toEqual({ model: "fable", effort: "medium" });
  expect(configOf(CLAUDE, { model: "fable", effort: "medium" })).toEqual({
    model: "fable",
    effort: "medium",
  });
  expect(configOf(CODEX, { model: "gpt-6-luna", effort: "" })).toEqual({ model: "gpt-6-luna" });
  expect(configOf(CLAUDE, { model: "", effort: "" })).toBeUndefined();
});

test("a pick stands until a turn starts with it, or a pick_not_used comes after it; a turn ending leaves it", () => {
  const pick = { model: "fable", effort: "low", notices: 4 };
  const opus = { model: { id: "opus", name: "Opus" }, effort: { id: "high", name: "High" } };
  expect(pickStands(pick, opus, 4)).toBe(true);
  expect(
    pickStands(
      pick,
      { model: { id: "fable", name: "Fable" }, effort: { id: "low", name: "Low" } },
      4,
    ),
  ).toBe(false);
  // the model taken, the effort not: it stands
  expect(pickStands(pick, { model: { id: "fable", name: "Fable" }, effort: opus.effort }, 4)).toBe(
    true,
  );
  expect(pickStands(pick, opus, 9)).toBe(false);
});

test("an account's own list, as the host lists it: in its order, each by its name, ids never read", () => {
  // another sign-in gets another list: a 1M-context Fable, and the host's order, not sorted by name or version
  const account: AgentModels = {
    model_option: "model",
    effort_option: "effort",
    current_model: "claude-fable-5-1[1m]",
    current_effort: "medium",
    models: [
      { id: "claude-fable-5-1[1m]", name: "Fable 5.1 [1m]", efforts: LMH },
      { id: "claude-opus-5-5", name: "Opus 5.5", efforts: LMH },
      { id: "claude-haiku-5-5", name: "Haiku 5.5" },
      { id: "claude-fable-5-1", name: "Fable 5.1", efforts: LMH },
    ],
  };
  const [model] = menusOf("en", draftShown(account, undefined, undefined)!, () => {});
  expect(model!.options).toEqual(account.models.map((m) => ({ value: m.id, name: m.name })));
  expect(model!.value).toBe("claude-fable-5-1[1m]");
});
