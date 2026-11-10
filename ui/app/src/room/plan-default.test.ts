// D-014, D-043: a new conversation's default plan weighs sign-in (agent.ts's agentFor, with plan-default.ts's inputs):
// the lawyer's choice; the matter's plan if signed in; the plan last asked with if signed in; the only plan signed in;
// else the order without sign-in. And the plan last asked with, kept in localStorage; the signed-in kinds, from the
// setup store's state.
import { expect, test } from "vitest";
import { agentFor, DEFAULT_AGENT, type Used } from "./agent.ts";
import type { AgentSummary } from "./host-next.ts";
import { lastPlan, LAST_PLAN_KEY, rememberPlan, signedKinds } from "./plan-default.ts";
import type { SetupState } from "../store/setup.ts";

const LISTED: AgentSummary[] = [
  { id: "claude-acp", kind: "claude" },
  { id: "codex-acp", kind: "codex" },
];
const CLAUDE_ONLY = new Set(["claude"] as const);
const CODEX_ONLY = new Set(["codex"] as const);
const BOTH = new Set(["claude", "codex"] as const);
const NONE = new Set<never>();
const claudeThread: Used = { thread: "t1", agent: "claude-acp", last_activity: 1 };
const codexThread: Used = { thread: "t2", agent: "codex-acp", last_activity: 2 };
const base = { chosen: null, threads: [] as Used[], agents: LISTED };

test("a new matter with only ChatGPT signed in defaults to ChatGPT, never to Claude", () => {
  expect(agentFor({ ...base, signed: CODEX_ONLY })).toBe("codex-acp");
  // before the host lists agents, the canonical id
  expect(agentFor({ ...base, agents: [], signed: CODEX_ONLY })).toBe("codex-acp");
});

test("the matter's plan wins while signed in, and gives way to the signed-in one when it isn't", () => {
  expect(agentFor({ ...base, threads: [codexThread], signed: BOTH, last: "claude-acp" })).toBe(
    "codex-acp",
  );
  expect(agentFor({ ...base, threads: [codexThread], signed: CLAUDE_ONLY })).toBe("claude-acp");
});

test("with both signed in and no matter plan, the plan last asked with wins; with none, Claude", () => {
  expect(agentFor({ ...base, signed: BOTH, last: "codex-acp" })).toBe("codex-acp");
  expect(agentFor({ ...base, signed: BOTH, last: "claude-acp" })).toBe("claude-acp");
  expect(agentFor({ ...base, signed: BOTH })).toBe(DEFAULT_AGENT);
});

test("the plan last asked with counts only while signed in and listed", () => {
  expect(agentFor({ ...base, signed: CLAUDE_ONLY, last: "codex-acp" })).toBe("claude-acp");
  expect(agentFor({ ...base, agents: [LISTED[0]!], signed: BOTH, last: "codex-acp" })).toBe(
    "claude-acp",
  );
});

test("with none signed in, the order without sign-in stands, so the gate offers the sign-in", () => {
  expect(agentFor({ ...base, signed: NONE })).toBe(DEFAULT_AGENT);
  expect(agentFor({ ...base, threads: [codexThread], signed: NONE })).toBe("codex-acp");
  expect(agentFor({ ...base, threads: [claudeThread], signed: null, last: "codex-acp" })).toBe(
    "claude-acp",
  );
});

test("the lawyer's choice is kept, signed in or not", () => {
  expect(agentFor({ ...base, chosen: "claude-acp", signed: CODEX_ONLY })).toBe("claude-acp");
});

test("the plan last asked with is kept in localStorage, and a broken storage keeps nothing", () => {
  const kept = new Map<string, string>();
  const storage = {
    getItem: (k: string) => kept.get(k) ?? null,
    setItem: (k: string, v: string) => void kept.set(k, v),
  };
  expect(lastPlan(storage)).toBeNull();
  rememberPlan("codex-acp", storage);
  expect(kept.get(LAST_PLAN_KEY)).toBe("codex-acp");
  expect(lastPlan(storage)).toBe("codex-acp");
  const broken = {
    getItem: () => {
      throw new Error("denied");
    },
    setItem: () => {
      throw new Error("denied");
    },
  };
  expect(lastPlan(broken)).toBeNull();
  expect(() => rememberPlan("codex-acp", broken)).not.toThrow();
});

test("the signed-in kinds: null before any sign-in state, then the kinds signed in", () => {
  const state = (agents: SetupState["agents"]) => ({ agents }) as SetupState;
  const signedIn = { signed_in: { method: "plan", source: "own", at: 1 } } as const;
  expect(signedKinds(state({}))).toBeNull();
  expect(signedKinds(state({ "claude-acp": { install: { ready: { version: "1" } } } }))).toBeNull();
  expect([...(signedKinds(state({ "claude-acp": { signIn: { signed_out: {} } } })) ?? [])]).toEqual(
    [],
  );
  expect([
    ...(signedKinds(
      state({ "claude-acp": { signIn: { signed_out: {} } }, "codex-acp": { signIn: signedIn } }),
    ) ?? []),
  ]).toEqual(["codex"]);
});
