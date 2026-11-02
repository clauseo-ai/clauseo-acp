// The agent a new conversation runs, fixed at `create_thread` by its first message (HANDOFF item 9), and the plan name a
// row shows for it. The order, first that is given: the lawyer's choice in the composer (R15's ListAgents gives the
// options); the "Start new conversations with" setting (H7's default-agent key, which U11 sets; not on the wire yet,
// so null today); the agent of the matter's newest thread; else DEFAULT_AGENT. Agent ids are the settings' keys
// (`agents.<id>`), which R15 renames from `claude` to `claude-acp`, so nothing here keys on an id but DEFAULT_AGENT,
// and the plan names go by the id's start.
import type { AgentId, ThreadSummary } from "../transport/protocol.ts";
import type { AgentInfo } from "./host-next.ts";

/** The agent when nothing else says: the e2e settings' `agents.claude` (ui/e2e/app-windows/scratch.ts's AGENT_ID).
 * When `claude-acp` reaches settings, this and AGENT_ID change in one PR. */
export const DEFAULT_AGENT: AgentId = "claude";

/** What decides a new conversation's agent, strongest first. */
export type AgentInputs = {
  /** The lawyer's choice in the composer for this conversation, if they made one. */
  chosen: AgentId | null;
  /** The "Start new conversations with" setting (H7), if the host gives it. */
  setting: AgentId | null;
  /** The matter's threads, archived ones too: the newest names the agent the lawyer used last here. */
  threads: readonly Pick<ThreadSummary, "thread" | "agent">[];
};

/** The agent `create_thread` names. */
export function agentFor({ chosen, setting, threads }: AgentInputs): AgentId {
  if (chosen !== null) return chosen;
  if (setting !== null) return setting;
  let newest: Pick<ThreadSummary, "thread" | "agent"> | null = null;
  // ids are v7 UUIDs, whose text sorts by creation
  for (const t of threads) if (newest === null || t.thread > newest.thread) newest = t;
  return newest?.agent ?? DEFAULT_AGENT;
}

/** The plan a row names for an agent: Claude Code's ids begin `claude`, Codex's `codex`. An id that is neither is
 * shown as it is, in English. */
export function planName(agent: AgentId): string {
  if (agent.startsWith("claude")) return "Claude";
  if (agent.startsWith("codex")) return "ChatGPT";
  return agent;
}

/** The plan a matter's rows are measured against (rail-conversations.tsx's `plan`): its oldest thread's, by id. A row
 * names its plan only where it differs. DESIGN.md:1154 says the first conversation's, HANDOFF item 9 the most common;
 * U6 takes the first, the one the host knows. */
export function matterPlan(
  threads: readonly Pick<ThreadSummary, "thread" | "agent">[],
): string | undefined {
  let oldest: Pick<ThreadSummary, "thread" | "agent"> | null = null;
  for (const t of threads) if (oldest === null || t.thread < oldest.thread) oldest = t;
  return oldest === null ? undefined : planName(oldest.agent);
}

/** The composer's plan choice for a new conversation, from the agents the host lists (R15): one option per agent that
 * is set up, labelled by its plan; none when there is only one to choose. */
export function planOptions(agents: readonly AgentInfo[]): { value: AgentId; label: string }[] {
  const options = agents.map((a) => ({ value: a.agent, label: planName(a.agent) }));
  return options.length > 1 ? options : [];
}
