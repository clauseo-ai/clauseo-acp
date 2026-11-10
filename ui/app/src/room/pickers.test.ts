// pickers.ts's rules: which of an agent's options the pickers show, the value a draft shows, and what create_thread
// sends as config.
import { expect, test } from "vitest";
import type { ConfigOption } from "../transport/protocol.ts";
import { draftConfig, draftValue, pickables, setOption } from "./pickers.ts";

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
