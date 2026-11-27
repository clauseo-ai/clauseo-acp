// @vitest-environment happy-dom
// Code mode, said once (code-mode.ts): "Code mode on" at the foot of the composer's open model menu, nothing to press.
// ChatGPT always; Claude on a 5.5-or-newer model, or when its conversation's last finished answer made a script call;
// absent otherwise, with no warning. The menu fits the window, its foot pinned under its scrolling rows. No answer
// carries a mark of its own, and the status line says nothing of it.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { act } from "react";
import { afterEach, expect, test } from "vitest";
import { agentText, header, reset, step, userMessage } from "../../testing/stream-script.ts";
import type { AgentModels, ConfigOption, ViewEntry } from "../../transport/protocol.ts";
import { fakeNext } from "../fake-next.ts";
import type { AgentSummary } from "../host-next.ts";
import { claudeCodeModeModel, codeModeLine, modelVersion } from "./code-mode.ts";
import { mountRoom, THE_MATTER, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const ON = "Code mode on";
const M = (id: string, name: string, description?: string) => ({
  id,
  name,
  ...(description === undefined ? {} : { description }),
});

test("a model's version, from its name, its description or its id; a date or 1M is none", () => {
  expect(modelVersion(M("opus-5-5", "Opus 5.5"))).toEqual([5, 5]);
  expect(modelVersion(M("claude-opus-4-5-20251101", "claude-opus-4-5-20251101"))).toEqual([4, 5]);
  expect(modelVersion(M("x", "Opus 6"))).toEqual([6, 0]);
  expect(modelVersion(M("default", "Default (recommended)"))).toBeNull();
  expect(modelVersion(M("sonnet[1m]", "Sonnet (1M context)"))).toBeNull();
  expect(modelVersion(M("default", "Default (recommended)", "Opus 5.5 · Most capable"))).toEqual([
    5, 5,
  ]);
});

test("a Claude model runs in code mode by its version alone from 5.5 on", () => {
  expect(claudeCodeModeModel(M("claude-haiku-5-5", "Haiku 5.5"))).toBe(true);
  expect(claudeCodeModeModel(M("sonnet-5-5", "Sonnet 5.5"))).toBe(true);
  expect(claudeCodeModeModel(M("opus-6", "Opus 6"))).toBe(true);
  expect(claudeCodeModeModel(M("fable-5-2", "Fable 5.2"))).toBe(false);
  expect(claudeCodeModeModel(M("opus-4-7", "Opus 4.7"))).toBe(false);
  expect(claudeCodeModeModel(M("default", "Default (recommended)"))).toBe(false);
});

test("the line's truth: ChatGPT always, Claude by its last answer, else by the model; no other agent", () => {
  const new55 = M("opus-5-5", "Opus 5.5");
  const old = M("opus-4-7", "Opus 4.7");
  expect(codeModeLine("codex", undefined, undefined)).toBe(true);
  expect(codeModeLine("codex", false, old)).toBe(true);
  expect(codeModeLine("claude", true, old)).toBe(true);
  // a step's false only says no script call was made: the model decides
  expect(codeModeLine("claude", false, new55)).toBe(true);
  expect(codeModeLine("claude", false, old)).toBe(false);
  expect(codeModeLine("claude", undefined, new55)).toBe(true);
  expect(codeModeLine("claude", undefined, old)).toBe(false);
  expect(codeModeLine("claude", undefined, undefined)).toBe(false);
  expect(codeModeLine(undefined, true, new55)).toBe(false);
});

const level = (id: string) => ({ id, name: id[0]!.toUpperCase() + id.slice(1) });
const LMH = ["low", "medium", "high"].map(level);
const models = (effort: string, list: [string, string][]): AgentModels => ({
  model_option: "model",
  effort_option: effort,
  current_model: list[0]![0],
  current_effort: "medium",
  models: list.map(([id, name]) => ({ id, name, efforts: LMH, default_effort: "medium" })),
});
const CLAUDE: AgentSummary = {
  id: "claude-acp",
  kind: "claude",
  models: models("effort", [
    ["opus-5-5", "Opus 5.5"],
    ["opus-4-7", "Opus 4.7"],
  ]),
};
const CODEX: AgentSummary = {
  id: "codex-acp",
  kind: "codex",
  models: models("reasoning_effort", [["gpt-6.2-sol", "gpt-6.2-sol"]]),
};
const next = () => fakeNext({ matters: () => [], agents: [CLAUDE, CODEX], parts: ["agents"] });
const select = (id: string, category: string, current: string): ConfigOption =>
  ({
    id,
    name: id,
    category,
    kind: { select: { current_value: current, options: [{ value: current, name: current }] } },
  }) as ConfigOption;

/** A conversation of `agent`, idle after one answer whose step says `codeMode`. */
function conversation(
  agent: string,
  codeMode: boolean | undefined,
  claude = "opus-4-7",
): ViewEntry[] {
  const model = agent === "codex-acp" ? "gpt-6.2-sol" : claude;
  const effort = agent === "codex-acp" ? "reasoning_effort" : "effort";
  const head: ViewEntry = {
    header: {
      ...header("idle").header!,
      agent,
      config_options: [select("model", "model", model), select(effort, "thought_level", "medium")],
    },
  };
  return [
    head,
    userMessage(5, 1, "Summarise the notice."),
    agentText(6, 1, "It gives sixty days."),
    step(7, codeMode === undefined ? {} : { code_mode: codeMode }),
  ];
}

/** Opens the model menu and reads its foot; closes it again. */
async function foot(): Promise<string | null> {
  // the menus come once the host's agents are in, which the file's first room waits on
  for (let i = 0; i < 20 && h!.one("pick-model") === null; i += 1) await h!.settle();
  await h!.click(h!.one("pick-model"));
  await h!.settle();
  const said = document.querySelector('[data-testid="pick-model-foot"]')?.textContent ?? null;
  const menu = document.querySelector<HTMLElement>('[role="menu"]');
  await act(async () => {
    menu?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  });
  await h!.settle();
  return said;
}

async function open(agent: string, codeMode: boolean | undefined, claude?: string) {
  h = await mountRoom({ next: next() });
  const [head, ...rest] = conversation(agent, codeMode, claude);
  h.deliver([reset(rest, head)]);
  await h.settle();
}

test("ChatGPT: the model menu says code mode is on, whatever its answers' steps say", async () => {
  await open("codex-acp", undefined);
  expect(await foot()).toBe(ON);
});

test("Claude: as its last finished answer ran, even on a model older than 5.5", async () => {
  await open("claude-acp", true);
  expect(await foot()).toBe(ON);
  await h!.unmount();
  await open("claude-acp", false);
  expect(await foot()).toBeNull();
  // no warning in its place
  expect(document.body.textContent).not.toContain("Code mode");
});

test("an existing Claude conversation on Opus 5.5 says code mode is on, whatever its last step's code_mode", async () => {
  // a loaded history whose last answer made a script call
  await open("claude-acp", true, "opus-5-5");
  expect(await foot()).toBe(ON);
  await h!.unmount();
  // a plain answer (no script call), and one finished before the field: the model decides
  await open("claude-acp", false, "opus-5-5");
  expect(await foot()).toBe(ON);
  await h!.unmount();
  await open("claude-acp", undefined, "opus-5-5");
  expect(await foot()).toBe(ON);
});

test("in a short window the menu fits: its rows scroll inside it, the foot pinned under them", async () => {
  await open("claude-acp", true, "opus-5-5");
  for (let i = 0; i < 20 && h!.one("pick-model") === null; i += 1) await h!.settle();
  await h!.click(h!.one("pick-model"));
  await h!.settle();
  const menu = document.querySelector<HTMLElement>('[role="menu"]')!;
  const said = document.querySelector<HTMLElement>('[data-testid="pick-model-foot"]')!;
  const rows = menu.querySelector<HTMLElement>(".room-pick-rows")!;
  // the menu is capped to the room Base UI measures and to the window, and does not scroll itself
  expect(menu.classList).toContain("max-h-available");
  expect(menu.classList).toContain("overflow-hidden");
  expect(menu.classList).toContain("flex-col");
  expect(menu.classList).not.toContain("overflow-y-auto");
  const primitives = readFileSync(
    join(import.meta.dirname, "../../../../components/src/styles/primitives.css"),
    "utf8",
  );
  expect(primitives).toMatch(
    /@utility max-h-available \{\s*max-height: min\(var\(--available-height, 100dvh\), calc\(100dvh - 2 \* var\(--s2\)\)\);/,
  );
  // the rows scroll and may shrink; the foot sits outside them and never shrinks
  expect(rows.classList).toContain("overflow-y-auto");
  expect(rows.classList).toContain("min-h-0");
  expect(rows.contains(said)).toBe(false);
  expect(said.parentElement).toBe(menu);
  expect(said.classList).toContain("flex-none");
  expect(rows.querySelectorAll('[role="menuitemradio"]').length).toBeGreaterThan(0);
});

test("Claude before any answer: by the model picked, 5.5 on and not older", async () => {
  h = await mountRoom({ matters: [THE_MATTER], threads: [], next: next() });
  await h.settle();
  expect(await foot()).toBe(ON);
  await h.click(h.one("pick-model"));
  await h.settle();
  const older = [...document.querySelectorAll<HTMLElement>('[role="menuitemradio"]')].find((i) =>
    i.textContent?.startsWith("Opus 4.7"),
  );
  await h.click(older);
  await h.settle();
  expect(await foot()).toBeNull();
});

test("no answer carries a code mode mark, and the status line says nothing of it", async () => {
  await open("claude-acp", true);
  expect(h!.all("answer-actions")).toHaveLength(1);
  expect(h!.one("answer-code-mode")).toBeNull();
  expect(h!.host.querySelector(".room-tag")).toBeNull();
  expect(h!.host.querySelector(".room-status")?.textContent ?? "").not.toContain("Code mode");
});
