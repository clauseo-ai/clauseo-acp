// @vitest-environment happy-dom
// The model and effort on the composer's status line, in the real room over FakeTransport (config-menus.ts,
// pickers.ts): a new conversation's from its plan's options as the agent last reported them, the matter's remembered
// picks preselected, a pick sent with create_thread; a conversation's from its header, a pick sent as
// set_config_option and shown at once, the header's later options followed with no error; no effort without the
// agent's thought_level option, and no menus at all without options. Real menus: a named button, the chosen value
// checked, Escape closes it, and axe finds nothing.
import axe from "axe-core";
import { act } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { header, reset, THREAD, upsert } from "../testing/stream-script.ts";
import type { ConfigOption, MatterSummary, ThreadId, ViewEntry } from "../transport/protocol.ts";
import { fakeNext } from "./fake-next.ts";
import type { AgentSummary } from "./host-next.ts";
import { defaultOutcome, mountRoom, THE_MATTER, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  vi.restoreAllMocks();
});

const MADE: ThreadId = "00000000-0000-7000-8000-0000000000aa";

const select = (
  id: string,
  name: string,
  category: string,
  current: string,
  values: [string, string, string?][],
): ConfigOption =>
  ({
    id,
    name,
    category,
    kind: {
      select: {
        current_value: current,
        options: values.map(([value, label, description]) => ({
          value,
          name: label,
          ...(description === undefined ? {} : { description }),
        })),
      },
    },
  }) as ConfigOption;

const MODEL = select("model", "Model", "model", "opus", [
  ["opus", "Opus", "For the hardest questions"],
  ["sonnet", "Sonnet"],
  ["haiku", "Haiku"],
]);
const EFFORT = select("effort", "Effort", "thought_level", "high", [
  ["low", "Low"],
  ["medium", "Medium"],
  ["high", "High"],
]);
const MODE = select("mode", "Mode", "mode", "default", [["default", "Default"]]);

const claude = (config_options?: ConfigOption[]): AgentSummary => ({
  id: "claude-acp",
  kind: "claude",
  ...(config_options === undefined ? {} : { config_options }),
});

const withAgents = (agents: AgentSummary[]) =>
  fakeNext({ matters: () => [], agents, parts: ["agents"] });

const REMEMBERED: MatterSummary = {
  ...THE_MATTER,
  agent_prefs: { claude: { model: "sonnet", effort: "low" } },
};

const trigger = (id: string) => h!.one(`pick-${id}`) as HTMLButtonElement | null;
const said = (id: string) => trigger(id)?.textContent?.trim() ?? null;
const items = () => [...document.querySelectorAll<HTMLElement>('[role="menuitemradio"]')];
const item = (name: string) => items().find((i) => i.textContent?.startsWith(name)) ?? null;
const menu = () => document.querySelector<HTMLElement>('[role="menu"]');

/** Opens a menu from its trigger, as a click does. */
async function open(id: string) {
  await h!.click(trigger(id));
  await h!.settle();
}

/** A header with the agent's options, as the host sends it for the open thread. */
const headerWith = (config_options: ConfigOption[]): ViewEntry => {
  const base = header();
  return { header: { ...base.header!, config_options } };
};

test("a new conversation shows the model and effort the matter remembers, and a pick goes with create_thread", async () => {
  h = await mountRoom({
    matters: [REMEMBERED],
    threads: [],
    next: withAgents([claude([MODE, MODEL, EFFORT])]),
    dispatch: (command) =>
      command.create_thread !== undefined
        ? { thread_created: { thread: MADE } }
        : defaultOutcome(command),
  });
  // the remembered picks preselected; the mode is never offered
  expect(said("model")).toBe("Sonnet");
  expect(said("effort")).toBe("Low");
  expect(trigger("mode")).toBeNull();
  expect(trigger("model")?.getAttribute("aria-label")).toBe("Model: Sonnet");
  await open("model");
  expect(menu()).not.toBeNull();
  expect(item("Sonnet")?.getAttribute("aria-checked")).toBe("true");
  // the agent's own description, under its name
  expect(item("Opus")?.textContent).toContain("For the hardest questions");
  await h.click(item("Haiku"));
  await h.settle();
  expect(said("model")).toBe("Haiku");
  // nothing reached the host for the pick: a draft sends nothing until its first question
  expect(h.sent("set_config_option")).toEqual([]);
  await h.type("What does clause nine say about notice?");
  await h.key({ key: "Enter", keyCode: 13 });
  await h.settle();
  expect(h.sent("create_thread")).toEqual([
    { matter: THE_MATTER.matter, agent: "claude-acp", config: { model: "haiku", effort: "low" } },
  ]);
});

test("with no pick in the draft create_thread sends no config: the host applies the remembered picks itself", async () => {
  h = await mountRoom({
    matters: [REMEMBERED],
    threads: [],
    next: withAgents([claude([MODEL, EFFORT])]),
    dispatch: (command) =>
      command.create_thread !== undefined
        ? { thread_created: { thread: MADE } }
        : defaultOutcome(command),
  });
  await h.type("Summarise the indemnity clause");
  await h.key({ key: "Enter", keyCode: 13 });
  await h.settle();
  expect(h.sent("create_thread")).toEqual([{ matter: THE_MATTER.matter, agent: "claude-acp" }]);
});

test("a remembered value the agent no longer offers falls back to the agent's own, with nothing said", async () => {
  h = await mountRoom({
    matters: [{ ...THE_MATTER, agent_prefs: { claude: { model: "retired-model" } } }],
    threads: [],
    next: withAgents([claude([MODEL, EFFORT])]),
  });
  expect(said("model")).toBe("Opus");
  expect(said("effort")).toBe("High");
  expect(h.host.textContent).not.toContain("retired-model");
});

test("no thought_level option, no effort; no options reported yet, no menus and no placeholder word", async () => {
  h = await mountRoom({ threads: [], next: withAgents([claude([MODEL])]) });
  expect(said("model")).toBe("Opus");
  expect(trigger("effort")).toBeNull();
  await h.unmount();
  h = await mountRoom({ threads: [], next: withAgents([claude()]) });
  expect(trigger("model")).toBeNull();
  expect(trigger("effort")).toBeNull();
  expect(h.host.querySelector(".room-status")?.textContent ?? "").not.toContain("Default");
});

test("in a conversation: the header's options, the plan fixed, a pick sent as set_config_option and shown at once", async () => {
  h = await mountRoom({ next: withAgents([claude([MODEL, EFFORT])]) });
  h.deliver([reset([], headerWith([MODEL, EFFORT]))]);
  await h.settle();
  // the plan is fixed: no plan switch, its words stand, the menus after them
  expect(h.host.querySelector(".room-status [role=group]")).toBeNull();
  expect(h.host.querySelector(".room-status-plan")?.textContent).toBe("Uses your Claude plan");
  expect(said("model")).toBe("Opus");
  await open("effort");
  await h.click(item("Medium"));
  await h.settle();
  expect(h.sent("set_config_option")).toEqual([
    { thread: THREAD, option_id: "effort", value: { select: "medium" } },
  ]);
  expect(said("effort")).toBe("Medium");
  // the agent took it: the header says so, and the menu reads it
  const took = select("effort", "Effort", "thought_level", "medium", [
    ["low", "Low"],
    ["medium", "Medium"],
    ["high", "High"],
  ]);
  h.deliver([upsert(headerWith([MODEL, took]))]);
  await h.settle();
  expect(said("effort")).toBe("Medium");
});

test("a pick the host refuses, or the agent replaces, gives way to the options as they are, with no error", async () => {
  let refuse = true;
  h = await mountRoom({
    next: withAgents([claude([MODEL, EFFORT])]),
    dispatch: (command) => {
      if (command.set_config_option !== undefined && refuse)
        throw { code: "agent.config_invalid", message: "not offered" };
      return defaultOutcome(command);
    },
  });
  vi.spyOn(console, "warn").mockImplementation(() => undefined);
  h.deliver([reset([], headerWith([MODEL, EFFORT]))]);
  await h.settle();
  await open("model");
  await h.click(item("Haiku"));
  await h.settle();
  expect(said("model")).toBe("Opus");
  expect(h.host.querySelector('[role="alert"]')).toBeNull();
  // accepted, then the agent's options come back on another value (the host fell back): the menu follows them
  refuse = false;
  await open("model");
  await h.click(item("Haiku"));
  await h.settle();
  expect(said("model")).toBe("Haiku");
  const fellBack = select("model", "Model", "model", "sonnet", [
    ["opus", "Opus"],
    ["sonnet", "Sonnet"],
  ]);
  h.deliver([upsert(headerWith([fellBack, EFFORT]))]);
  await h.settle();
  expect(said("model")).toBe("Sonnet");
  expect(h.host.querySelector('[role="alert"]')).toBeNull();
});

test("a real menu: a named button that opens it, its title, axe finds nothing, and Escape closes it", async () => {
  h = await mountRoom({ threads: [], next: withAgents([claude([MODEL, EFFORT])]) });
  const t = trigger("model")!;
  expect(t.tagName).toBe("BUTTON");
  expect(t.getAttribute("aria-haspopup")).toBe("menu");
  await act(async () => t.focus());
  // Enter or Space on a button is its click
  await act(async () => t.click());
  await h.settle();
  expect(menu()).not.toBeNull();
  expect(t.getAttribute("aria-expanded")).toBe("true");
  expect(menu()?.textContent).toContain("Model");
  // the open menu and the status line it opens from; landmarks are the page's (the menu sits in the layer), and Base
  // UI's focus guards around a popup are its own
  const rules = { region: { enabled: false }, "color-contrast": { enabled: false } };
  for (const el of [menu()!, h.host.querySelector<HTMLElement>(".room-status")!]) {
    const context = { include: [el], exclude: ["[data-base-ui-focus-guard]"] };
    const results = await axe.run(context, { resultTypes: ["violations"], rules });
    expect(results.violations.map((v) => v.id)).toEqual([]);
  }
  await act(async () => {
    menu()!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  });
  await h.settle();
  expect(t.getAttribute("aria-expanded")).not.toBe("true");
});
