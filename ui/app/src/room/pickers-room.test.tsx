// @vitest-environment happy-dom
// The model and effort on the composer's status line, in the real room over FakeTransport (config-menus.ts,
// pickers.ts): a new conversation's from its plan's models (AgentSummary.models), the matter's remembered picks
// preselected, a pick sent with create_thread under the agent's own keys; every model listed, each model's own
// efforts, the effort kept or defaulted on a change of model; a quiet loading line until the models are in, never a
// made-up default; a conversation's from its header, a pick sent as set_config_option and shown at once, the header's
// later options followed with no error. Real menus: a named button, the chosen value checked, Escape closes it, and
// axe finds nothing. The other plan held for the next message (held-agent.ts). Code mode's line at the model menu's
// foot: thread/code-mode.test.tsx.
import axe from "axe-core";
import { act } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { header, reset, THREAD, upsert } from "../testing/stream-script.ts";
import type {
  AgentModels,
  ConfigOption,
  MatterSummary,
  ThreadId,
  ViewEntry,
} from "../transport/protocol.ts";
import { fakeNext } from "./fake-next.ts";
import type { AgentSummary } from "./host-next.ts";
import { defaultOutcome, mountRoom, summary, THE_MATTER, type Harness } from "./thread/harness.ts";

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

const level = (id: string) => ({ id, name: id[0]!.toUpperCase() + id.slice(1) });
const LMH = ["low", "medium", "high"].map(level);

/** Claude's models as the probe fills them: Opus takes three efforts, Sonnet two, Haiku none. */
const MODELS: AgentModels = {
  model_option: "model",
  effort_option: "effort",
  current_model: "opus",
  current_effort: "high",
  models: [
    {
      id: "opus",
      name: "Opus",
      description: "For the hardest questions",
      efforts: LMH,
      default_effort: "medium",
    },
    {
      id: "sonnet",
      name: "Sonnet",
      efforts: [level("low"), level("medium")],
      default_effort: "medium",
    },
    { id: "haiku", name: "Haiku", efforts: [] },
  ],
};

/** ChatGPT's, its effort under its own key. */
const CODEX_MODELS: AgentModels = {
  model_option: "model",
  effort_option: "reasoning_effort",
  current_model: "gpt-6.2-sol",
  current_effort: "medium",
  models: [
    { id: "gpt-6.2-sol", name: "gpt-6.2-sol", efforts: LMH, default_effort: "medium" },
    { id: "gpt-6-luna", name: "gpt-6-luna", efforts: LMH, default_effort: "low" },
  ],
};

/** The session's options as a conversation's header holds them: the model, the effort, and a mode never drawn. */
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

const claude = (models?: AgentModels): AgentSummary => ({
  id: "claude-acp",
  kind: "claude",
  ...(models === undefined ? {} : { models }),
});
const codex = (models?: AgentModels): AgentSummary => ({
  id: "codex-acp",
  kind: "codex",
  ...(models === undefined ? {} : { models }),
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
const line = () => h!.host.querySelector(".room-status")?.textContent ?? "";

/** Closes the open menu, as Escape does. */
async function closeMenu() {
  await act(async () => {
    menu()!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  });
  await h!.settle();
}

/** Opens a menu from its trigger, as a click does. */
async function open(id: string) {
  await h!.click(trigger(id));
  await h!.settle();
}

/** A header with the agent's options, as the host sends it for the open thread. */
const headerWith = (
  config_options: ConfigOption[],
  status: "idle" | "running" = "idle",
  agent = "claude-acp",
  more: Partial<NonNullable<ViewEntry["header"]>> = {},
): ViewEntry => {
  const base = header(status);
  return { header: { ...base.header!, agent, config_options, ...more } };
};

/** What a turn on Opus at High runs with, as its TurnStarted sets the header. */
const RAN_OPUS = {
  model: { id: "opus", name: "Opus" },
  effort: { id: "high", name: "High" },
};

const making = (command: Parameters<typeof defaultOutcome>[0]) =>
  command.create_thread !== undefined
    ? { thread_created: { thread: MADE } }
    : defaultOutcome(command);

/** Asks a question from the composer, as Enter does. */
async function ask(text: string) {
  await h!.type(text);
  await h!.key({ key: "Enter", keyCode: 13 });
  await h!.settle();
}

test("a new conversation shows the model and effort the matter remembers, and a pick goes with create_thread", async () => {
  h = await mountRoom({
    matters: [REMEMBERED],
    threads: [],
    next: withAgents([claude(MODELS)]),
    dispatch: making,
  });
  // the remembered picks preselected; the mode is never offered
  expect(said("model")).toBe("Sonnet");
  expect(said("effort")).toBe("Low");
  expect(trigger("mode")).toBeNull();
  expect(trigger("model")?.getAttribute("aria-label")).toBe("Model: Sonnet");
  await open("model");
  expect(menu()).not.toBeNull();
  expect(item("Sonnet")?.getAttribute("aria-checked")).toBe("true");
  // the agent's own description, under its name; none for a model the agent describes not
  expect(item("Opus")?.textContent).toBe("OpusFor the hardest questions");
  expect(item("Sonnet")?.textContent).toBe("Sonnet");
  await h.click(item("Opus"));
  await h.settle();
  // Opus takes the Low shown: kept
  expect(said("model")).toBe("Opus");
  expect(said("effort")).toBe("Low");
  // nothing reached the host for the pick: a draft sends nothing until its first question
  expect(h.sent("set_config_option")).toEqual([]);
  await ask("What does clause nine say about notice?");
  expect(h.sent("create_thread")).toEqual([
    { matter: THE_MATTER.matter, agent: "claude-acp", config: { model: "opus", effort: "low" } },
  ]);
});

test("with no pick in the draft create_thread sends no config: the host applies the remembered picks itself", async () => {
  h = await mountRoom({
    matters: [REMEMBERED],
    threads: [],
    next: withAgents([claude(MODELS)]),
    dispatch: making,
  });
  await ask("Summarise the indemnity clause");
  expect(h.sent("create_thread")).toEqual([{ matter: THE_MATTER.matter, agent: "claude-acp" }]);
});

test("each model its own efforts: a change of model keeps the effort it takes, else takes its default", async () => {
  h = await mountRoom({ threads: [], next: withAgents([claude(MODELS)]), dispatch: making });
  expect(said("model")).toBe("Opus");
  expect(said("effort")).toBe("High");
  await open("model");
  await h.click(item("Sonnet"));
  await h.settle();
  // Sonnet has no High: its own default
  expect(said("effort")).toBe("Medium");
  await open("effort");
  expect(items().map((i) => i.textContent)).toEqual(["Low", "Medium"]);
  await h.click(item("Low"));
  await h.settle();
  // back to Opus: Low is kept, as Opus takes it
  await open("model");
  await h.click(item("Opus"));
  await h.settle();
  expect(said("effort")).toBe("Low");
  // Haiku takes none: no effort menu, and none sent
  await open("model");
  await h.click(item("Haiku"));
  await h.settle();
  expect(trigger("effort")).toBeNull();
  await ask("What does clause nine say about notice?");
  expect(h.sent("create_thread")).toEqual([
    { matter: THE_MATTER.matter, agent: "claude-acp", config: { model: "haiku" } },
  ]);
});

test("ChatGPT's picks go under its own keys, and its whole list is offered", async () => {
  const many: AgentModels = {
    ...CODEX_MODELS,
    models: [
      ...CODEX_MODELS.models,
      ...Array.from({ length: 10 }, (_, i) => ({
        id: `gpt-5.${i}`,
        name: `gpt-5.${i}`,
        efforts: LMH,
      })),
    ],
  };
  h = await mountRoom({
    threads: [],
    next: withAgents([claude(MODELS), codex(many)]),
    dispatch: making,
  });
  await h.click(h.host.querySelector(".room-status-seg button:nth-of-type(2)"));
  await h.settle();
  expect(said("model")).toBe("gpt-6.2-sol");
  expect(said("reasoning_effort")).toBe("Medium");
  await open("model");
  expect(items()).toHaveLength(12);
  await h.click(item("gpt-6-luna"));
  await h.settle();
  await ask("Draft a reply to the notice");
  expect(h.sent("create_thread")).toEqual([
    {
      matter: THE_MATTER.matter,
      agent: "codex-acp",
      config: { model: "gpt-6-luna", reasoning_effort: "medium" },
    },
  ]);
});

test("an account's own list renders as the host gives it, in its order, each row its name", async () => {
  const account: AgentModels = {
    ...MODELS,
    current_model: "claude-fable-5-1[1m]",
    models: [
      { id: "claude-fable-5-1[1m]", name: "Fable 5.1 [1m]", efforts: LMH },
      { id: "claude-opus-5-5", name: "Opus 5.5", efforts: LMH },
      { id: "claude-haiku-5-5", name: "Haiku 5.5" },
      { id: "claude-fable-5-1", name: "Fable 5.1", efforts: LMH },
    ],
  };
  h = await mountRoom({ threads: [], next: withAgents([claude(account)]) });
  expect(said("model")).toBe("Fable 5.1 [1m]");
  await open("model");
  expect(items().map((i) => i.textContent)).toEqual([
    "Fable 5.1 [1m]",
    "Opus 5.5",
    "Haiku 5.5",
    "Fable 5.1",
  ]);
});

test("a remembered value the agent no longer offers falls back to what a new session starts on, with nothing said", async () => {
  h = await mountRoom({
    matters: [{ ...THE_MATTER, agent_prefs: { claude: { model: "retired-model" } } }],
    threads: [],
    next: withAgents([claude(MODELS)]),
  });
  expect(said("model")).toBe("Opus");
  expect(said("effort")).toBe("High");
  expect(h.host.textContent).not.toContain("retired-model");
});

test("until the agent's models are in, the line says they are loading, never a default", async () => {
  h = await mountRoom({ threads: [], next: withAgents([claude()]) });
  expect(trigger("model")).toBeNull();
  expect(trigger("effort")).toBeNull();
  expect(line()).toContain("Loading models…");
  expect(line()).not.toContain("default");
  expect(line()).not.toContain("Default");
  await h.unmount();
  // a conversation with no session's options and no models yet: the same
  h = await mountRoom({ next: withAgents([claude()]) });
  h.deliver([reset([], headerWith([]))]);
  await h.settle();
  expect(line()).toContain("Loading models…");
});

const next = () =>
  h!.host.querySelector(".room-status")?.textContent?.match(/Next message: [^·]+(· \S+)?/)?.[0];

test("in a conversation a pick stays in the composer, named for the next message, and goes with that send", async () => {
  h = await mountRoom({ next: withAgents([claude(MODELS)]) });
  h.deliver([reset([], headerWith([MODE, MODEL, EFFORT]))]);
  await h.settle();
  // no record yet: the session's options; the plan's words stand, no segmented switch
  expect(h.host.querySelector(".room-status [role=group]")).toBeNull();
  expect(said("model")).toBe("Opus");
  expect(said("effort")).toBe("High");
  expect(trigger("mode")).toBeNull();
  await open("model");
  await h.click(item("Sonnet"));
  await h.settle();
  // nothing goes to the host on a pick
  expect(h.sent("set_config_option")).toEqual([]);
  // the triggers keep naming what the session holds; the pick is ticked and named for the next message
  expect(said("model")).toBe("Opus");
  expect(line()).toContain("Next message: Sonnet · Medium");
  await open("model");
  expect(item("Sonnet")?.getAttribute("aria-checked")).toBe("true");
  await closeMenu();
  await open("effort");
  // Sonnet's efforts, Medium (its default, as Sonnet has no High) ticked
  expect(items().map((i) => i.textContent)).toEqual(["Low", "Medium"]);
  expect(item("Medium")?.getAttribute("aria-checked")).toBe("true");
  await closeMenu();
  await ask("And the notice clause?");
  // the model and its effort together, with the message
  expect(h.sent("send_prompt").at(-1)).toMatchObject({
    config: { model: "sonnet", effort: "medium" },
  });
});

test("with no pick standing a send carries no config: the host keeps the session's own", async () => {
  h = await mountRoom({ next: withAgents([claude(MODELS)]) });
  h.deliver([reset([], headerWith([MODEL, EFFORT], "idle", "claude-acp", RAN_OPUS))]);
  await h.settle();
  await ask("Summarise the indemnity clause");
  expect("config" in h.sent("send_prompt").at(-1)!).toBe(false);
});

test("a conversation with no session yet names what its last turn ran with, from the header", async () => {
  h = await mountRoom({ next: withAgents([claude(MODELS)]) });
  h.deliver([
    reset(
      [],
      headerWith([], "idle", "claude-acp", {
        model: { id: "sonnet", name: "Sonnet" },
        effort: { id: "low", name: "Low" },
      }),
    ),
  ]);
  await h.settle();
  expect(said("model")).toBe("Sonnet");
  expect(said("effort")).toBe("Low");
  await open("model");
  await h.click(item("Opus"));
  await h.settle();
  // Opus takes the Low shown: kept, for the next message
  expect(h.sent("set_config_option")).toEqual([]);
  expect(said("model")).toBe("Sonnet");
  expect(line()).toContain("Next message: Opus · Low");
  await ask("Draft the reply");
  expect(h.sent("send_prompt").at(-1)).toMatchObject({ config: { model: "opus", effort: "low" } });
});

test("a pick mid-answer: the line keeps the record, the pick named for the next message, and it goes with the next send", async () => {
  h = await mountRoom({ next: withAgents([claude(MODELS)]) });
  h.deliver([reset([], headerWith([MODEL, EFFORT], "running", "claude-acp", RAN_OPUS))]);
  await h.settle();
  await open("effort");
  await h.click(item("Medium"));
  await h.settle();
  expect(said("effort")).toBe("High");
  expect(line()).toContain("Next message: Opus · Medium");
  // the answer ends: the pick stays
  h.deliver([upsert(headerWith([MODEL, EFFORT], "idle", "claude-acp", RAN_OPUS))]);
  await h.settle();
  expect(said("effort")).toBe("High");
  expect(line()).toContain("Next message: Opus · Medium");
  await ask("Then the indemnity");
  expect(h.sent("send_prompt").at(-1)).toMatchObject({
    config: { model: "opus", effort: "medium" },
  });
  // the next turn starts with it: the record names it, and the pick is spent
  h.deliver([
    upsert(
      headerWith([MODEL, EFFORT], "running", "claude-acp", {
        ...RAN_OPUS,
        effort: { id: "medium", name: "Medium" },
      }),
    ),
  ]);
  await h.settle();
  expect(said("effort")).toBe("Medium");
  expect(next()).toBeUndefined();
});

test("a steer carries the picks; after it joins the answer the pick stays and the line keeps the record", async () => {
  h = await mountRoom({ next: withAgents([claude(MODELS)]) });
  h.deliver([
    reset(
      [],
      headerWith([MODEL, EFFORT], "running", "claude-acp", { ...RAN_OPUS, steer_joins_turn: true }),
    ),
  ]);
  await h.settle();
  await open("model");
  await h.click(item("Sonnet"));
  await h.settle();
  await ask("And the notice clause?");
  await h.type("Check clause 14 too");
  await h.click(h.one("steer-send-now"));
  await h.settle();
  const sent = h.sent("send_prompt");
  expect(sent.map((s) => s.delivery)).toEqual(["steer", "send_now"]);
  for (const s of sent) expect(s.config).toEqual({ model: "sonnet", effort: "medium" });
  // the steer joined the running answer, which keeps its model: the record stands, the pick stays for the next message
  expect(said("model")).toBe("Opus");
  expect(said("effort")).toBe("High");
  expect(line()).toContain("Next message: Sonnet · Medium");
});

test("a pick the agent couldn't use: the host's pick_not_used clears it, and the line shows what ran", async () => {
  h = await mountRoom({ next: withAgents([claude(MODELS)]) });
  h.deliver([reset([], headerWith([MODEL, EFFORT], "idle", "claude-acp", RAN_OPUS))]);
  await h.settle();
  await open("model");
  await h.click(item("Haiku"));
  await h.settle();
  expect(line()).toContain("Next message: Haiku");
  await ask("Summarise clause 9");
  expect(h.sent("send_prompt").at(-1)).toMatchObject({ config: { model: "haiku" } });
  // the turn starts on Opus at High, with the notice that the pick wasn't used
  h.deliver([
    upsert(headerWith([MODEL, EFFORT], "running", "claude-acp", RAN_OPUS)),
    upsert({
      notice: {
        order: 40,
        event: "00000000-0000-7000-8000-0000000000ee",
        level: "info",
        text: "Clauseo couldn't switch to the model or effort you picked for this answer, so it used Opus at High effort.",
        kind: { pick_not_used: { model: "Opus", effort: "High" } },
        ts: 40_000,
      },
    }),
  ]);
  await h.settle();
  expect(said("model")).toBe("Opus");
  expect(said("effort")).toBe("High");
  expect(next()).toBeUndefined();
});

test("an option the composer doesn't know (ChatGPT's collaboration_mode, fast mode) draws nothing", async () => {
  const collaboration = select("collaboration_mode", "Collaboration mode", "mode", "default", [
    ["default", "Default"],
    ["plan", "Plan"],
  ]);
  const odd = select("fast_mode", "Fast mode", "fast_mode", "off", [
    ["off", "Off"],
    ["on", "On"],
  ]);
  const options = [collaboration, odd, MODEL, EFFORT];
  h = await mountRoom({ next: withAgents([claude(MODELS)]) });
  h.deliver([reset([], headerWith(options))]);
  await h.settle();
  expect(said("model")).toBe("Opus");
  expect(said("effort")).toBe("High");
  expect(trigger("collaboration_mode")).toBeNull();
  expect(trigger("fast_mode")).toBeNull();
  expect(h.host.querySelectorAll(".room-status-menu")).toHaveLength(2);
  for (const word of ["Collaboration", "Plan to", "Fast", "Off"])
    expect(line()).not.toContain(word);
});

// issue 11: while an answer runs the status line stays over the working line, naming the model and effort in use
// from the record, and a pick there is for the next message
test.each([
  ["Claude", "claude-acp", "Uses your Claude plan"],
  ["ChatGPT", "codex-acp", "Uses your ChatGPT plan"],
] as const)(
  "while %s answers, its model and effort stay named over the working line, and a pick is for the next message",
  async (_, agent, plan) => {
    const thread = { ...summary(), agent };
    h = await mountRoom({ threads: [thread], next: withAgents([claude(MODELS), codex(MODELS)]) });
    h.deliver([reset([], headerWith([MODEL, EFFORT], "running", agent, RAN_OPUS))]);
    await h.settle();
    expect(h.one("cancel")).not.toBeNull();
    expect(h.one("working")?.textContent).toContain("Answering.");
    const status = h.host.querySelector<HTMLElement>(".room-status");
    expect(status?.hidden).toBe(false);
    expect(status?.querySelector(".room-status-plan")?.textContent).toBe(plan);
    expect(said("model")).toBe("Opus");
    expect(said("effort")).toBe("High");
    expect(trigger("model")?.disabled).toBe(false);
    await open("effort");
    await h.click(item("Medium"));
    await h.settle();
    expect(h.sent("set_config_option")).toEqual([]);
    // the record, not the pick, names what runs
    expect(said("effort")).toBe("High");
    expect(line()).toContain("Next message: Opus · Medium");
    // the answer done: the line as it always stood, the pick still for the next message
    h.deliver([upsert(headerWith([MODEL, EFFORT], "idle", agent, RAN_OPUS))]);
    await h.settle();
    expect(h.one("cancel")).toBeNull();
    expect(h.host.querySelector<HTMLElement>(".room-status")?.hidden).toBe(false);
    expect(said("model")).toBe("Opus");
    expect(line()).toContain("Next message: Opus · Medium");
  },
);

test("a real menu: a named button that opens it, its title, axe finds nothing, and Escape closes it", async () => {
  h = await mountRoom({ threads: [], next: withAgents([claude(MODELS)]) });
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

/** Picks a plan from the conversation's plan menu (a select over the plan's words). */
async function holdPlan(name: string) {
  await h!.click(h!.host.querySelector(".room-status-plan"));
  await h!.settle();
  const option = [...document.querySelectorAll<HTMLElement>('[role="option"]')].find(
    (o) => o.textContent === name,
  );
  await h!.click(option);
  await h!.settle();
}

test("the other plan held: named before sending with its own model and effort, a way back, and sent with the next message", async () => {
  h = await mountRoom({ next: withAgents([claude(MODELS), codex(CODEX_MODELS)]) });
  h.deliver([reset([], headerWith([MODEL, EFFORT]))]);
  await h.settle();
  await holdPlan("ChatGPT");
  // nothing went to the host: the hold waits for the next message
  expect(h.sent("set_config_option")).toEqual([]);
  expect(h.host.querySelector(".room-status-plan")?.textContent).toBe(
    "ChatGPT from your next message",
  );
  expect(said("model")).toBe("gpt-6.2-sol");
  expect(said("reasoning_effort")).toBe("Medium");
  const back = h.host.querySelector<HTMLButtonElement>(".room-status-back button");
  expect(back?.textContent).toBe("Back to Claude");
  await h.click(back);
  await h.settle();
  expect(h.host.querySelector(".room-status-plan")?.textContent).toBe("Uses your Claude plan");
  expect(said("model")).toBe("Opus");
  expect(h.host.querySelector(".room-status-back")).toBeNull();
  // held again, its effort changed there, then sent: the agent and its picks go with the message, and the hold stays
  // until the header names ChatGPT
  await holdPlan("ChatGPT");
  await open("reasoning_effort");
  await h.click(item("High"));
  await h.settle();
  expect(h.sent("set_config_option")).toEqual([]);
  await ask("Now check the reply against clause 14");
  expect(h.sent("send_prompt").at(-1)).toMatchObject({
    thread: THREAD,
    agent: "codex-acp",
    config: { model: "gpt-6.2-sol", reasoning_effort: "high" },
  });
  // the host took it: the line names ChatGPT's plan from the send, with its picks
  expect(h.host.querySelector(".room-status-plan")?.textContent).toBe("Uses your ChatGPT plan");
  expect(said("reasoning_effort")).toBe("High");
});

test("a switch queued behind Claude's answer keeps ChatGPT's menus and picks until the header names ChatGPT", async () => {
  h = await mountRoom({ next: withAgents([claude(MODELS), codex(CODEX_MODELS)]) });
  h.deliver([reset([], headerWith([MODEL, EFFORT], "running"))]);
  await h.settle();
  await holdPlan("ChatGPT");
  const config = { model: "gpt-6.2-sol", reasoning_effort: "medium" };
  await ask("Now check the reply against clause 14");
  expect(h.sent("send_prompt").at(-1)).toMatchObject({ agent: "codex-acp", config });
  // the host took it, queued behind Claude's answer: the line names ChatGPT's plan, and the next message goes there too
  expect(said("model")).toBe("gpt-6.2-sol");
  expect(h.host.querySelector(".room-status-plan")?.textContent).toBe("Uses your ChatGPT plan");
  await ask("And against clause 15");
  expect(h.sent("send_prompt")).toHaveLength(2);
  expect(h.sent("send_prompt").at(-1)).toMatchObject({ agent: "codex-acp", config });
  // the queued switch starts: the header names ChatGPT, and the pick has done its work
  h.deliver([upsert(headerWith([], "running", "codex-acp"))]);
  await h.settle();
  expect(h.host.querySelector(".room-status-back")).toBeNull();
  expect(line()).not.toContain("from your next message");
});

test("picking the conversation's own plan drops the hold; a message with nothing held names no agent", async () => {
  h = await mountRoom({ next: withAgents([claude(MODELS), codex(CODEX_MODELS)]) });
  h.deliver([reset([], headerWith([MODEL, EFFORT]))]);
  await h.settle();
  await holdPlan("ChatGPT");
  await holdPlan("Claude");
  expect(h.host.querySelector(".room-status-back")).toBeNull();
  await ask("What does clause nine say?");
  const sent = h.sent("send_prompt").at(-1) as Record<string, unknown>;
  expect(sent["agent"]).toBeUndefined();
  // no pick stands, so no config goes with it either (#1319)
  expect(sent["config"]).toBeUndefined();
});

test("a message sent into a running answer with a plan held carries the agent and its model and effort", async () => {
  h = await mountRoom({ next: withAgents([claude(MODELS), codex(CODEX_MODELS)]) });
  h.deliver([
    reset([], headerWith([MODEL, EFFORT], "running", "claude-acp", { steer_joins_turn: true })),
  ]);
  await h.settle();
  await holdPlan("ChatGPT");
  await open("model");
  await h.click(item("gpt-6-luna"));
  await h.settle();
  await ask("Now check the reply against clause 14");
  const sent = h.sent("send_prompt").at(-1) as Record<string, unknown>;
  expect(sent["delivery"]).toBe("steer");
  expect(sent["agent"]).toBe("codex-acp");
  expect(sent["config"]).toEqual({ model: "gpt-6-luna", reasoning_effort: "medium" });
});

test("a held plan's change of model sends its effort with it", async () => {
  h = await mountRoom({ next: withAgents([claude(MODELS), codex(CODEX_MODELS)]) });
  h.deliver([reset([], headerWith([MODEL, EFFORT]))]);
  await h.settle();
  await holdPlan("ChatGPT");
  await open("model");
  await h.click(item("gpt-6-luna"));
  await h.settle();
  // gpt-6-luna takes the Medium shown: kept, and sent with the model
  expect(said("reasoning_effort")).toBe("Medium");
  await ask("Draft the reply");
  expect(h.sent("send_prompt").at(-1)).toMatchObject({
    agent: "codex-acp",
    config: { model: "gpt-6-luna", reasoning_effort: "medium" },
  });
});
