// pickers.ts's rules: which of an agent's options the pickers show, the value a draft shows, and what create_thread
// sends as config.
import { expect, test } from "vitest";
import type { ConfigOption } from "../transport/protocol.ts";
import { menusOf } from "./config-menus.ts";
import { draftConfig, draftValue, pickables, resolvedDefault, setOption } from "./pickers.ts";

const select = (id: string, category: string, current: string, values: string[]): ConfigOption =>
  ({
    id,
    name: id,
    category,
    kind: {
      select: { current_value: current, options: values.map((v) => ({ value: v, name: v })) },
    },
  }) as ConfigOption;

const MODEL = select("model", "model", "opus", ["opus", "sonnet", "haiku"]);
const EFFORT = select("effort", "thought_level", "high", ["low", "medium", "high"]);
const MODE = select("mode", "mode", "default", ["default", "plan"]);

test("the model, then the effort; a mode, an option with no values and no options at all show nothing", () => {
  expect(pickables([MODE, EFFORT, MODEL]).map((p) => p.id)).toEqual(["model", "effort"]);
  expect(pickables([MODEL]).map((p) => p.category)).toEqual(["model"]);
  expect(pickables([select("model", "model", "x", [])])).toEqual([]);
  expect(pickables([MODE])).toEqual([]);
  expect(pickables(undefined)).toEqual([]);
  expect(pickables([])).toEqual([]);
});

test("a draft shows its pick, else the matter's remembered value, else the agent's; each only while offered", () => {
  const [model] = pickables([MODEL]);
  expect(draftValue(model!, "haiku", "sonnet")).toBe("haiku");
  expect(draftValue(model!, undefined, "sonnet")).toBe("sonnet");
  expect(draftValue(model!, "gone", "also-gone")).toBe("opus");
  expect(draftValue(model!)).toBe("opus");
});

test("create_thread's config: none without a pick; a pick of the model alone keeps the remembered effort", () => {
  const options = [MODEL, EFFORT, MODE];
  expect(draftConfig(options, undefined, { model: "sonnet" })).toBeUndefined();
  expect(draftConfig(options, {}, { model: "sonnet" })).toBeUndefined();
  // a pick the options no longer offer is no pick
  expect(draftConfig(options, { model: "gone" }, undefined)).toBeUndefined();
  expect(draftConfig(options, { model: "haiku" }, { effort: "low" })).toEqual({
    model: "haiku",
    effort: "low",
  });
  // the pickers' options only: a remembered mode is the host's to apply, never sent from here
  expect(draftConfig(options, { effort: "medium" }, { mode: "plan" })).toEqual({
    model: "opus",
    effort: "medium",
  });
});

test("a pick in a conversation is set_config_option with a select value", () => {
  expect(setOption("t-1", "model", "haiku")).toEqual({
    set_config_option: { thread: "t-1", option_id: "model", value: { select: "haiku" } },
  });
});

/** claude-agent-acp 0.89.1's shape (agent-options.json): a Default row on each, the model's naming what it resolves to
 * in its description, the effort's naming nothing. */
const named = (
  id: string,
  category: string,
  current: string,
  values: [string, string, string?][],
) =>
  ({
    id,
    name: id,
    category,
    kind: {
      select: {
        current_value: current,
        options: values.map(([value, name, description]) => ({
          value,
          name,
          ...(description === undefined ? {} : { description }),
        })),
      },
    },
  }) as ConfigOption;
const CLAUDE_MODEL = named("model", "model", "default", [
  ["default", "Default (recommended)", "Opus 5.5"],
  ["opus", "Opus 5.5", "For complex work and everyday tasks"],
  ["sonnet", "Sonnet 5.5", "Most efficient for simpler tasks"],
]);
const CLAUDE_EFFORT = named("effort", "thought_level", "default", [
  ["default", "Default"],
  ["low", "Low"],
  ["medium", "Medium"],
]);
const shownOf = (options: ConfigOption[], value?: string) => {
  const menus = menusOf(
    "en",
    pickables(options),
    (p) => value ?? p.current,
    () => {},
  );
  return menus.map((m) => {
    const o = m.options.find((x) => x.value === m.value)!;
    return { name: o.name, note: o.note, description: o.description };
  });
};

test("the default resolved: the agent's model by its own name, with (default) as the note and no description", () => {
  const [model] = pickables([CLAUDE_MODEL]);
  expect(resolvedDefault(model!)).toBe("Opus 5.5");
  expect(shownOf([CLAUDE_MODEL])).toEqual([
    { name: "Opus 5.5", note: "(default)", description: undefined },
  ]);
});

test("the default unresolved: the agent's own name for it, with no note", () => {
  const [effort] = pickables([CLAUDE_EFFORT]);
  expect(resolvedDefault(effort!)).toBeUndefined();
  expect(shownOf([CLAUDE_EFFORT])).toEqual([
    { name: "Default", note: undefined, description: undefined },
  ]);
  // a Default row with an empty description names nothing either
  const blank = named("model", "model", "default", [["default", "Default", " "]]);
  expect(shownOf([blank])).toEqual([{ name: "Default", note: undefined, description: " " }]);
});

test("an explicit pick shows as the agent names it; the menu still marks the default row", () => {
  expect(shownOf([CLAUDE_MODEL], "sonnet")).toEqual([
    { name: "Sonnet 5.5", note: undefined, description: "Most efficient for simpler tasks" },
  ]);
  const [menu] = menusOf(
    "en",
    pickables([CLAUDE_MODEL]),
    () => "sonnet",
    () => {},
  );
  expect(menu!.options.map((o) => [o.name, o.note])).toEqual([
    ["Opus 5.5", "(default)"],
    ["Opus 5.5", undefined],
    ["Sonnet 5.5", undefined],
  ]);
  // ChatGPT's values are all concrete, with no Default row: nothing is marked
  expect(shownOf([MODEL, EFFORT])).toEqual([
    { name: "opus", note: undefined, description: undefined },
    { name: "high", note: undefined, description: undefined },
  ]);
  expect(
    menusOf(
      "hi",
      pickables([CLAUDE_MODEL]),
      () => "default",
      () => {},
    )[0]!.options[0]!.note,
  ).toBe("(डिफ़ॉल्ट)");
});
