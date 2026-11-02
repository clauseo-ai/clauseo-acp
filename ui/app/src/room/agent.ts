// The agent a new conversation runs, fixed at `create_thread` by its first message (HANDOFF item 9), and the plan name a
// row shows for it. The order, first that is given: the lawyer's choice in the composer (R15's ListAgents gives the
// options); the "Start new conversations with" setting (H7's default-agent key, which U11 sets; not on the wire yet,
// so null today); the agent of the matter's newest thread; else DEFAULT_AGENT. Agent ids are the settings' keys
// (`agents.<id>`), which R15 renames from `claude` to `claude-acp`, so nothing here keys on an id but DEFAULT_AGENT,
// and the plan names go by the agent's kind (R15's `AgentSummary`), read off the id's start only for an agent the list
// doesn't hold.
import type { AgentId, AgentKind, ThreadSummary } from "../transport/protocol.ts";
import type { AgentSummary } from "./host-next.ts";

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

/** Each kind's plan, as the rows and the composer's choice name it. */
const PLAN: Record<AgentKind, string> = { claude: "Claude", codex: "ChatGPT" };

/** An agent's kind: the one the host lists for it (R15's ListAgents), else read off its id (Claude Code's begin
 * `claude`, Codex's `codex`), for an agent the list doesn't hold or lists with no kind (#154 gives a kind to
 * `claude-acp` and `codex-acp` only, so today's `claude` lists without one); none for any other id. */
function kindOf(agent: AgentId, agents: readonly AgentSummary[]): AgentKind | undefined {
  const listed = agents.find((a) => a.id === agent);
  if (listed?.kind !== undefined) return listed.kind;
  if (agent.startsWith("claude")) return "claude";
  if (agent.startsWith("codex")) return "codex";
  return undefined;
}

/** The plan a row names for an agent, by its kind: "Claude" or "ChatGPT". An agent of no kind is shown by its id, in
 * English. */
export function planName(agent: AgentId, agents: readonly AgentSummary[] = []): string {
  const kind = kindOf(agent, agents);
  return kind === undefined ? agent : PLAN[kind];
}

/** The plan a matter's rows are measured against (rail-conversations.tsx's `plan`): its oldest thread's, by id. A row
 * names its plan only where it differs. DESIGN.md:1154 says the first conversation's, HANDOFF item 9 the most common;
 * U6 takes the first, the one the host knows. */
export function matterPlan(
  threads: readonly Pick<ThreadSummary, "thread" | "agent">[],
  agents: readonly AgentSummary[] = [],
): string | undefined {
  let oldest: Pick<ThreadSummary, "thread" | "agent"> | null = null;
  for (const t of threads) if (oldest === null || t.thread < oldest.thread) oldest = t;
  return oldest === null ? undefined : planName(oldest.agent, agents);
}

/** The composer's plan choice for a new conversation, from the agents the host lists (R15) that have a kind (a plan
 * to name), each labelled by its kind's plan; none when there is only one to choose. */
export function planOptions(agents: readonly AgentSummary[]): { value: AgentId; label: string }[] {
  const options = agents
    .filter((a) => kindOf(a.id, agents) !== undefined)
    .map((a) => ({ value: a.id, label: planName(a.id, agents) }));
  return options.length > 1 ? options : [];
}
