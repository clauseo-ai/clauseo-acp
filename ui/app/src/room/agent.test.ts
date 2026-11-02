// agent.ts: the agent a new conversation runs, the plan name a row shows, the matter's plan, and the composer's plan options.
import { expect, test } from "vitest";
import type { AgentSummary } from "./host-next.ts";
import { agentFor, DEFAULT_AGENT, knownPlan, matterPlan, planName, planOptions } from "./agent.ts";

// v7 UUID strings: the greater string is the newer thread
const T1 = "00000000-0000-7000-8000-000000000001";
const T2 = "00000000-0000-7000-8000-000000000002";

test("agentFor: the chosen agent wins over the setting and the threads", () => {
  const threads = [{ thread: T2, agent: "gemini" }];
  expect(agentFor({ chosen: "codex-acp", setting: "claude-acp", threads })).toBe("codex-acp");
});

const T3 = "00000000-0000-7000-8000-000000000003";

test("agentFor: the plan the matter uses most wins over the newest thread's and over the setting", () => {
  const threads = [
    { thread: T1, agent: "codex-acp" },
    { thread: T2, agent: "codex-acp" },
    { thread: T3, agent: "claude-acp" },
  ];
  expect(agentFor({ chosen: null, setting: "claude-acp", threads })).toBe("codex-acp");
});

test("agentFor: a tie goes to the plan used most recently: by H6's last activity, else the newest thread", () => {
  const older = { thread: T1, agent: "codex-acp" };
  const newer = { thread: T2, agent: "claude-acp" };
  expect(agentFor({ chosen: null, setting: null, threads: [older, newer] })).toBe("claude-acp");
  expect(agentFor({ chosen: null, setting: null, threads: [newer, older] })).toBe("claude-acp");
  // the older thread used last
  const active = [
    { ...older, last_activity: 20 },
    { ...newer, last_activity: 10 },
  ];
  expect(agentFor({ chosen: null, setting: null, threads: active })).toBe("codex-acp");
});

test("agentFor: a matter with no conversation takes the setting, else DEFAULT_AGENT", () => {
  expect(agentFor({ chosen: null, setting: "codex-acp", threads: [] })).toBe("codex-acp");
  expect(agentFor({ chosen: null, setting: null, threads: [] })).toBe(DEFAULT_AGENT);
});

test("planName: Claude Code's ids are Claude, Codex's are ChatGPT, others as they are", () => {
  expect(planName("claude")).toBe("Claude");
  expect(planName("claude-acp")).toBe("Claude");
  expect(planName("codex")).toBe("ChatGPT");
  expect(planName("codex-acp")).toBe("ChatGPT");
  expect(planName("gemini")).toBe("gemini");
});

test("matterPlan: the plan used most; a tie goes to the one used most recently", () => {
  const most = [
    { thread: T1, agent: "codex-acp" },
    { thread: T2, agent: "codex-acp" },
    { thread: T3, agent: "claude-acp" },
  ];
  expect(matterPlan(most)).toBe("ChatGPT");
  const tie = [
    { thread: T2, agent: "claude-acp" },
    { thread: T1, agent: "codex-acp" },
  ];
  expect(matterPlan(tie)).toBe("Claude");
});

test("matterPlan: undefined with no threads", () => {
  expect(matterPlan([])).toBeUndefined();
});

test("planOptions: two agents give two options, labelled by plan", () => {
  const agents: AgentSummary[] = [
    { id: "claude-acp", kind: "claude" },
    { id: "codex-acp", kind: "codex" },
  ];
  expect(planOptions(agents)).toEqual([
    { value: "claude-acp", label: "Claude" },
    { value: "codex-acp", label: "ChatGPT" },
  ]);
});

test("planName goes by the listed kind, not the id; an agent of no kind is its id", () => {
  const agents: AgentSummary[] = [{ id: "work-plan", kind: "codex" }, { id: "fake-agent" }];
  expect(planName("work-plan", agents)).toBe("ChatGPT");
  expect(planName("fake-agent", agents)).toBe("fake-agent");
  // the one agent with a plan to name is no choice
  expect(planOptions(agents)).toEqual([]);
  expect(matterPlan([{ thread: T1, agent: "work-plan" }], agents)).toBe("ChatGPT");
});

test("planOptions: one agent is no choice, so none; no agents, none", () => {
  expect(planOptions([{ id: "claude-acp", kind: "claude" }])).toEqual([]);
  expect(planOptions([])).toEqual([]);
});

test("S4: a listed agent with no kind is named by its id's prefix", () => {
  expect(planName("claude", [{ id: "claude" }])).toBe("Claude");
  expect(planName("codex", [{ id: "codex" }])).toBe("ChatGPT");
});

test("S4: planOptions offers only agents with a plan to name", () => {
  expect(
    planOptions([{ id: "claude" }, { id: "codex-acp", kind: "codex" }, { id: "fake-agent" }]),
  ).toEqual([
    { value: "claude", label: "Claude" },
    { value: "codex-acp", label: "ChatGPT" },
  ]);
  expect(planOptions([{ id: "claude-acp", kind: "claude" }, { id: "fake-agent" }])).toEqual([]);
});

test("knownPlan: a kind's plan, and none for an agent of no kind", () => {
  expect(knownPlan("claude")).toBe("Claude");
  expect(knownPlan("work-plan", [{ id: "work-plan", kind: "codex" }])).toBe("ChatGPT");
  expect(knownPlan("fake-agent", [{ id: "fake-agent" }])).toBeNull();
});
