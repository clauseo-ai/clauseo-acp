// agent.ts: the agent a new conversation runs, the plan name a row shows, the matter's plan, and the composer's plan options.
import { expect, test } from "vitest";
import type { AgentSummary } from "./host-next.ts";
import {
  agentFor,
  DEFAULT_AGENT,
  knownPlan,
  matterPlan,
  mostUsed,
  planName,
  planOptions,
} from "./agent.ts";

// v7 UUID strings: the greater string is the newer thread
const T1 = "00000000-0000-7000-8000-000000000001";
const T2 = "00000000-0000-7000-8000-000000000002";

test("agentFor: the chosen agent wins over the setting and the threads", () => {
  const threads = [{ thread: T2, agent: "gemini" }];
  expect(agentFor({ chosen: "codex-acp", setting: "claude-acp", threads, agents: [] })).toBe(
    "codex-acp",
  );
});

const T3 = "00000000-0000-7000-8000-000000000003";
const T4 = "00000000-0000-7000-8000-000000000004";
const T5 = "00000000-0000-7000-8000-000000000005";
const T6 = "00000000-0000-7000-8000-000000000006";
const T7 = "00000000-0000-7000-8000-000000000007";

test("agentFor: the plan the matter uses most wins over the newest thread's and over the setting", () => {
  const threads = [
    { thread: T1, agent: "codex-acp" },
    { thread: T2, agent: "codex-acp" },
    { thread: T3, agent: "claude-acp" },
  ];
  expect(agentFor({ chosen: null, setting: "claude-acp", threads, agents: [] })).toBe("codex-acp");
});

test("agentFor: a tie goes to the plan used most recently: by H6's last activity, else the newest thread", () => {
  const older = { thread: T1, agent: "codex-acp" };
  const newer = { thread: T2, agent: "claude-acp" };
  expect(agentFor({ chosen: null, setting: null, threads: [older, newer], agents: [] })).toBe(
    "claude-acp",
  );
  expect(agentFor({ chosen: null, setting: null, threads: [newer, older], agents: [] })).toBe(
    "claude-acp",
  );
  // the older thread used last
  const active = [
    { ...older, last_activity: 20 },
    { ...newer, last_activity: 10 },
  ];
  expect(agentFor({ chosen: null, setting: null, threads: active, agents: [] })).toBe("codex-acp");
});

test("agentFor: a matter with no conversation takes the setting, else DEFAULT_AGENT", () => {
  expect(agentFor({ chosen: null, setting: "codex-acp", threads: [], agents: [] })).toBe(
    "codex-acp",
  );
  expect(agentFor({ chosen: null, setting: null, threads: [], agents: [] })).toBe(DEFAULT_AGENT);
});

// the kinds the host lists, as R15 gives them: claude-acp and codex-acp carry their kind
const LISTED: AgentSummary[] = [
  { id: "claude-acp", kind: "claude" },
  { id: "codex-acp", kind: "codex" },
];

test("mostUsed: uses count by kind, so claude and claude-acp count together and beat one codex id", () => {
  // by id, codex-acp (3) is most used; by kind, Claude's four uses beat ChatGPT's three
  const threads = [
    { thread: T1, agent: "codex-acp" },
    { thread: T2, agent: "codex-acp" },
    { thread: T3, agent: "codex-acp" },
    { thread: T4, agent: "claude" },
    { thread: T5, agent: "claude" },
    { thread: T6, agent: "claude-acp" },
    { thread: T7, agent: "claude-acp" },
  ];
  expect(mostUsed(threads, LISTED)).toBe("claude-acp");
  expect(agentFor({ chosen: null, setting: null, threads, agents: LISTED })).toBe("claude-acp");
});

test("mostUsed: the winning kind's agent is the listed one of it used most recently", () => {
  // Claude wins with three uses; the newest of them is `claude`, not `claude-acp`, which was used most; the settings
  // still hold both (an older file's `claude` lists with no kind)
  const threads = [
    { thread: T1, agent: "claude-acp" },
    { thread: T2, agent: "claude-acp" },
    { thread: T3, agent: "claude" },
  ];
  const both = [{ id: "claude" }, ...LISTED];
  expect(mostUsed(threads, both)).toBe("claude");
  expect(matterPlan(threads, both)).toBe("Claude");
  // with `claude` gone from the settings, the newest listed Claude agent
  expect(mostUsed(threads, LISTED)).toBe("claude-acp");
});

test("a matter from before R15's rename: its `claude` uses still count, and a new conversation runs claude-acp", () => {
  // every conversation ran `claude`, which the settings no longer hold; creating a thread on it would be refused
  // (command.invalid), so the Claude plan's listed agent runs instead, and the switch presses that one
  const threads = [
    { thread: T1, agent: "codex-acp" },
    { thread: T2, agent: "claude" },
    { thread: T3, agent: "claude" },
  ];
  expect(mostUsed(threads, LISTED)).toBe("claude-acp");
  expect(agentFor({ chosen: null, setting: null, threads, agents: LISTED })).toBe("claude-acp");
  expect(matterPlan(threads, LISTED)).toBe("Claude");
  expect(planOptions(LISTED, threads)).toEqual([
    { value: "claude-acp", label: "Claude" },
    { value: "codex-acp", label: "ChatGPT" },
  ]);
  // before the host's list arrives, nothing is known to be unlisted: the newest id stands
  expect(mostUsed(threads, [])).toBe("claude");
});

test("mostUsed: an unlisted agent of no kind wins nothing a conversation can run, so the fallback runs", () => {
  const threads = [
    { thread: T1, agent: "fake-agent" },
    { thread: T2, agent: "fake-agent" },
  ];
  expect(mostUsed(threads, LISTED)).toBeNull();
  expect(agentFor({ chosen: null, setting: null, threads, agents: LISTED })).toBe(DEFAULT_AGENT);
  // the rows still measure against the plan the matter used
  expect(matterPlan(threads, LISTED)).toBe("fake-agent");
});

test("mostUsed: a tie between kinds goes to the kind used most recently, whatever the ids' counts", () => {
  // codex-acp is the most used id (two uses), but Claude and ChatGPT have two uses each, and Claude's are newer
  const threads = [
    { thread: T1, agent: "codex-acp" },
    { thread: T2, agent: "codex-acp" },
    { thread: T3, agent: "claude-acp" },
    { thread: T4, agent: "claude" },
  ];
  // the newest Claude use is `claude`, which isn't listed, so Claude's listed agent
  expect(mostUsed(threads, LISTED)).toBe("claude-acp");
});

test("mostUsed: an agent of no kind counts on its own, by its id", () => {
  // fake-agent's two uses count for itself; the two Claude uses across two ids tie it, and they are the newer
  const threads = [
    { thread: T1, agent: "fake-agent" },
    { thread: T2, agent: "fake-agent" },
    { thread: T3, agent: "claude-acp" },
    { thread: T4, agent: "claude" },
  ];
  expect(mostUsed(threads, [{ id: "fake-agent" }, ...LISTED])).toBe("claude-acp");
  // alone, it is the most used
  expect(mostUsed(threads.slice(0, 2), [{ id: "fake-agent" }, ...LISTED])).toBe("fake-agent");
});

test("planName: Claude Code's ids are Claude, Codex's are ChatGPT, others as they are", () => {
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

test("planOptions: one option per kind, Claude first, the canonical id of each kind", () => {
  // the two Claude ids give one option, and Claude leads though the list has codex first
  const agents: AgentSummary[] = [
    { id: "codex-acp", kind: "codex" },
    { id: "claude", kind: "claude" },
    { id: "claude-acp", kind: "claude" },
  ];
  expect(planOptions(agents)).toEqual([
    { value: "claude-acp", label: "Claude" },
    { value: "codex-acp", label: "ChatGPT" },
  ]);
});

test("planOptions: a kind's value is the id it used most recently, else its canonical id, else its first listed id", () => {
  const agents: AgentSummary[] = [
    { id: "claude", kind: "claude" },
    { id: "claude-acp", kind: "claude" },
    { id: "codex-acp", kind: "codex" },
  ];
  // the Claude plan's most recent use was `claude`, so its value is `claude`, not the canonical id
  expect(planOptions(agents, [{ thread: T1, agent: "claude" }])).toEqual([
    { value: "claude", label: "Claude" },
    { value: "codex-acp", label: "ChatGPT" },
  ]);
  // with no thread, the canonical id
  expect(planOptions(agents)[0]?.value).toBe("claude-acp");
  // with neither a thread nor a canonical id, the first listed id of the kind
  const bare: AgentSummary[] = [
    { id: "claude-b", kind: "claude" },
    { id: "claude-a", kind: "claude" },
    { id: "codex", kind: "codex" },
  ];
  expect(planOptions(bare)).toEqual([
    { value: "claude-b", label: "Claude" },
    { value: "codex", label: "ChatGPT" },
  ]);
});

test("planOptions: one kind is no choice, however many ids it has", () => {
  expect(planOptions([{ id: "claude-acp", kind: "claude" }, { id: "claude" }])).toEqual([]);
});

test("agentFor: without DEFAULT_AGENT listed, the Claude plan's agent, else the first listed agent with a kind", () => {
  // the host lists no claude-acp: the Claude plan's agent starts the conversation, not the codex one
  const agents: AgentSummary[] = [
    { id: "codex-acp", kind: "codex" },
    { id: "claude-x", kind: "claude" },
  ];
  expect(agentFor({ chosen: null, setting: null, threads: [], agents })).toBe("claude-x");
  // no Claude plan: the first listed agent with a kind
  const codexOnly: AgentSummary[] = [{ id: "fake-agent" }, { id: "codex-acp", kind: "codex" }];
  expect(agentFor({ chosen: null, setting: null, threads: [], agents: codexOnly })).toBe(
    "codex-acp",
  );
  // no kind at all: the first listed
  const none: AgentSummary[] = [{ id: "fake-agent" }, { id: "gemini" }];
  expect(agentFor({ chosen: null, setting: null, threads: [], agents: none })).toBe("fake-agent");
  // listing DEFAULT_AGENT keeps it, and an empty list is no host answer, so it keeps it too
  expect(agentFor({ chosen: null, setting: null, threads: [], agents: LISTED })).toBe(
    DEFAULT_AGENT,
  );
  expect(agentFor({ chosen: null, setting: null, threads: [], agents: [] })).toBe(DEFAULT_AGENT);
});

test("planOptions: one agent is no choice, so none; no agents, none", () => {
  expect(planOptions([{ id: "claude-acp", kind: "claude" }])).toEqual([]);
  expect(planOptions([])).toEqual([]);
});

test("S4: a listed agent with no kind is named by its id's prefix", () => {
  expect(planName("claude-acp", [{ id: "claude-acp" }])).toBe("Claude");
  expect(planName("codex", [{ id: "codex" }])).toBe("ChatGPT");
});

test("S4: planOptions offers only agents with a plan to name", () => {
  expect(
    planOptions([{ id: "claude-acp" }, { id: "codex-acp", kind: "codex" }, { id: "fake-agent" }]),
  ).toEqual([
    { value: "claude-acp", label: "Claude" },
    { value: "codex-acp", label: "ChatGPT" },
  ]);
  expect(planOptions([{ id: "claude-acp", kind: "claude" }, { id: "fake-agent" }])).toEqual([]);
});

test("knownPlan: a kind's plan, and none for an agent of no kind", () => {
  expect(knownPlan("claude-acp")).toBe("Claude");
  expect(knownPlan("work-plan", [{ id: "work-plan", kind: "codex" }])).toBe("ChatGPT");
  expect(knownPlan("fake-agent", [{ id: "fake-agent" }])).toBeNull();
});
