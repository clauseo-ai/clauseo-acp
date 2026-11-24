// @vitest-environment happy-dom
// The model line on the composer's status line when the menus can't stand (config-menus.ts's missingModels), and the
// header's record (pickers.ts's threadShown), in the real room over FakeTransport: "Loading models…" while the host's
// probe runs, the unread line once it failed (AgentSummary.models_unread), "Models show after your first message."
// where the firm requires the sandbox (no probe runs there), the menus once the models are in; Ask enabled in every
// state. Each TurnStarted sets the header's model and effort and clears what it leaves out: the line then shows
// nothing for it, never the last one shown.
import { afterEach, expect, test } from "vitest";
import { header, reset, upsert } from "../testing/stream-script.ts";
import type {
  AgentModels,
  AgentSummary,
  ConfigOption,
  ProtectionView,
  ThreadId,
  ViewEntry,
} from "../transport/protocol.ts";
import { fakeNext } from "./fake-next.ts";
import { defaultOutcome, mountRoom, THE_MATTER, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
});

const MADE: ThreadId = "00000000-0000-7000-8000-0000000000ab";
const LMH = ["low", "medium", "high"].map((id) => ({
  id,
  name: id[0]!.toUpperCase() + id.slice(1),
}));
const MODELS: AgentModels = {
  model_option: "model",
  effort_option: "effort",
  current_model: "opus",
  current_effort: "high",
  models: [
    { id: "opus", name: "Opus", efforts: LMH, default_effort: "medium" },
    { id: "sonnet", name: "Sonnet", efforts: LMH, default_effort: "medium" },
  ],
};
const claude = (more: Partial<AgentSummary> = {}): AgentSummary => ({
  id: "claude-acp",
  kind: "claude",
  ...more,
});
const withAgents = (agents: AgentSummary[]) =>
  fakeNext({ matters: () => [], agents, parts: ["agents"] });

/** This computer protected, the firm requiring the sandbox. */
const REQUIRED: ProtectionView = {
  tier: "a",
  computer: { protected: { tier: "a" } },
  required: true,
  unprotected_auto_allowed: true,
};

const making = (command: Parameters<typeof defaultOutcome>[0]) =>
  command.create_thread !== undefined
    ? { thread_created: { thread: MADE } }
    : defaultOutcome(command);
const line = () => h!.host.querySelector(".room-status")?.textContent ?? "";
const trigger = (id: string) => h!.one(`pick-${id}`);
const said = (id: string) => trigger(id)?.textContent?.trim() ?? null;

const LOADING = "Loading models…";
const UNREAD = "Couldn't read the models yet. You can still ask.";
const SANDBOX = "Models show after your first message.";

test.each([
  ["loading", claude(), false, LOADING],
  ["unread", claude({ models_unread: true }), false, UNREAD],
  ["sandbox required", claude(), true, SANDBOX],
  ["filled", claude({ models: MODELS }), false, null],
] as const)(
  "a new conversation, models %s: the line says so, and Ask stays enabled",
  async (_, agent, required, words) => {
    h = await mountRoom({
      threads: [],
      next: withAgents([agent]),
      dispatch: making,
      ...(required ? { protection: { view: () => REQUIRED } } : {}),
    });
    await h.settle();
    for (const other of [LOADING, UNREAD, SANDBOX].filter((w) => w !== words))
      expect(line()).not.toContain(other);
    // the model menu, its trigger at rest until its chunk is in
    if (words === null) expect(h.host.querySelector(".room-status-menu")?.textContent).toBe("Opus");
    else {
      expect(line()).toContain(words);
      expect(trigger("model")).toBeNull();
    }
    await h.type("What does clause nine say about notice?");
    const ask = h.host.querySelector<HTMLButtonElement>(".room-field-ask");
    expect(ask?.disabled).toBe(false);
    expect(ask?.getAttribute("aria-disabled")).not.toBe("true");
    await h.key({ key: "Enter", keyCode: 13 });
    await h.settle();
    expect(h.sent("create_thread")).toHaveLength(1);
  },
);

const select = (id: string, category: string, current: string, values: string[]): ConfigOption =>
  ({
    id,
    name: id,
    category,
    kind: {
      select: {
        current_value: current,
        options: values.map((v) => ({ value: v, name: v[0]!.toUpperCase() + v.slice(1) })),
      },
    },
  }) as ConfigOption;
const OPTIONS = [
  select("model", "model", "opus", ["opus", "sonnet"]),
  select("effort", "thought_level", "high", ["low", "medium", "high"]),
];

const headerOf = (status: "idle" | "running", more: Record<string, unknown> = {}): ViewEntry => {
  const base = header(status);
  return { header: { ...base.header!, agent: "claude-acp", config_options: OPTIONS, ...more } };
};

test("a turn that names no effort, or no model, shows nothing for it, never the last one shown", async () => {
  h = await mountRoom({ next: withAgents([claude({ models: MODELS })]), matters: [THE_MATTER] });
  h.deliver([
    reset(
      [],
      headerOf("running", {
        model: { id: "sonnet", name: "Sonnet" },
        effort: { id: "low", name: "Low" },
      }),
    ),
  ]);
  await h.settle();
  expect(said("model")).toBe("Sonnet");
  expect(said("effort")).toBe("Low");
  // the next turn names its model only: the effort is cleared
  h.deliver([upsert(headerOf("running", { model: { id: "opus", name: "Opus" } }))]);
  await h.settle();
  expect(said("model")).toBe("Opus");
  expect(trigger("effort")).toBeNull();
  expect(line()).not.toContain("Low");
  // and one that names neither: nothing, not the options' values or the last turn's
  h.deliver([upsert(headerOf("running"))]);
  await h.settle();
  expect(trigger("model")).toBeNull();
  expect(trigger("effort")).toBeNull();
  expect(line()).not.toContain("Opus");
  expect(line()).not.toContain("Loading models…");
});
