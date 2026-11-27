// @vitest-environment happy-dom
// The model switch (model-switch.ts, ModelSwitchDivider; StepEntry.model, effort and agent from host #1349's fix): a
// quiet divider above a question whose answer ran on another model or effort than the answer before it of the same
// agent, naming only what changed; none at a change of agent (the handoff's job), none where a step lacks the fields,
// and a reload draws it again.
import { afterEach, expect, test } from "vitest";
import { agentText, header, reset, step, userMessage, uuid } from "../../testing/stream-script.ts";
import type { StepEntry, ViewEntry } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";
import { switchOf } from "./model-switch.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const OPUS = { id: "opus", name: "Opus 5.5" };
const SONNET = { id: "sonnet", name: "Sonnet 5.5" };
const HIGH = { id: "high", name: "High" };
const MEDIUM = { id: "medium", name: "Medium" };
const SOL = { id: "gpt-6.2-sol", name: "6.2 Sol" };

type Ran = Pick<StepEntry, "model" | "effort" | "agent">;
const CLAUDE = "claude-acp";

/** A question of its own turn, its answer, and its step with what it ran on. */
function exchange(k: number, ran: Partial<Ran>): ViewEntry[] {
  const turn = uuid(k, 4);
  const q = userMessage(k * 10, k, `Question ${k}`);
  const a = agentText(k * 10 + 1, k, `Answer ${k}.`);
  return [
    { user_message: { ...q.user_message!, turn } },
    { agent_text: { ...a.agent_text!, turn } },
    step(k * 10 + 9, { turn, ...ran }),
  ];
}

const at = (over: Partial<Ran>): StepEntry => step(1, over).step!;

test("same agent, model changed: names the models alone", () => {
  expect(
    switchOf(
      at({ agent: CLAUDE, model: OPUS, effort: HIGH }),
      at({ agent: CLAUDE, model: SONNET, effort: HIGH }),
    ),
  ).toEqual({ from: "Opus 5.5", to: "Sonnet 5.5" });
});

test("effort only: names the efforts alone; both changed names both", () => {
  expect(
    switchOf(
      at({ agent: CLAUDE, model: OPUS, effort: HIGH }),
      at({ agent: CLAUDE, model: OPUS, effort: MEDIUM }),
    ),
  ).toEqual({ from: "High", to: "Medium" });
  expect(
    switchOf(
      at({ agent: CLAUDE, model: OPUS, effort: HIGH }),
      at({ agent: CLAUDE, model: SONNET, effort: MEDIUM }),
    ),
  ).toEqual({ from: "Opus 5.5 High", to: "Sonnet 5.5 Medium" });
  expect(
    switchOf(
      at({ agent: CLAUDE, model: OPUS, effort: HIGH }),
      at({ agent: CLAUDE, model: OPUS, effort: HIGH }),
    ),
  ).toBeNull();
});

test("agent changed: none", () => {
  expect(
    switchOf(at({ agent: CLAUDE, model: OPUS }), at({ agent: "codex-acp", model: SOL })),
  ).toBeNull();
});

test("an old step without the fields: none", () => {
  expect(
    switchOf(at({ agent: CLAUDE }), at({ agent: CLAUDE, model: SONNET, effort: HIGH })),
  ).toBeNull();
  expect(switchOf(at({}), at({ model: SONNET }))).toBeNull();
  expect(switchOf(at({ model: OPUS }), at({ model: SONNET }))).toBeNull();
});

const VIEW = [
  ...exchange(1, { agent: CLAUDE, model: OPUS, effort: HIGH }),
  ...exchange(2, { agent: CLAUDE, model: OPUS, effort: HIGH }),
  ...exchange(3, { agent: CLAUDE, model: SONNET, effort: HIGH }),
  // a switch to ChatGPT: the handoff's divider, not this one
  ...exchange(4, { agent: "codex-acp", model: SOL }),
  // an old step: nothing to compare
  ...exchange(5, { agent: "codex-acp" }),
];

test("in the thread: one divider, just above the question whose answer changed model; a reload keeps it", async () => {
  h = await mountRoom();
  h.deliver([reset(VIEW, header())]);
  await h.settle();
  const check = (room: Harness) => {
    const dividers = room.all("model-switch-divider");
    expect(dividers.map((d) => d.textContent)).toEqual(["Opus 5.5 → Sonnet 5.5"]);
    expect(dividers[0]!.nextElementSibling).toBe(room.all("message-user")[2]!);
  };
  check(h);
  h.deliver([reset(VIEW, header())]);
  await h.settle();
  check(h);
});
