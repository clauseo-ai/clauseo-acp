// The agent a new conversation runs, fixed at `create_thread` by its first message (HANDOFF item 9), and the plan name a
// row shows for it. The order, first that is given (Rohan, 2026-11-02, HANDOFF item 9): the lawyer's choice in the
// composer (R15's ListAgents gives the options); the plan used most in the matter, a tie going to the one used most
// recently; in a matter with no conversation, the "Start new conversations with" setting (H7's default-agent key, which
// U11 sets; not on the wire yet, so null today); else DEFAULT_AGENT. A row names its plan only where it differs from the
// matter's, by the same rule. Agent ids are the settings' keys
// (`agents.<id>`), which R15 renames from `claude` to `claude-acp`, so nothing here keys on an id but DEFAULT_AGENT,
// and the plan names go by the agent's kind (R15's `AgentSummary`), read off the id's start only for an agent the list
// doesn't hold.
import type { AgentId, AgentKind, ThreadSummary, Timestamp } from "../transport/protocol.ts";
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
  /** The matter's conversations, archived ones left out. */
  threads: readonly Used[];
};

/** A conversation as the rule weighs it: its agent, and when it was used: H6's last activity, else its creation (v7
 * ids sort by it). */
export type Used = Pick<ThreadSummary, "thread" | "agent"> & { last_activity?: Timestamp };

/** Whether `a` was used after `b`. */
const later = (a: Used, b: Used): boolean => {
  const at = [a.last_activity ?? -Infinity, b.last_activity ?? -Infinity] as const;
  if (at[0] !== at[1]) return at[0] > at[1];
  return a.thread > b.thread;
};

/** The agent the matter uses most, a tie going to the one used most recently; null with no conversation. */
export function mostUsed(threads: readonly Used[]): AgentId | null {
  const uses = new Map<AgentId, { n: number; last: Used }>();
  for (const t of threads) {
    const u = uses.get(t.agent);
    if (u === undefined) uses.set(t.agent, { n: 1, last: t });
    else uses.set(t.agent, { n: u.n + 1, last: later(t, u.last) ? t : u.last });
  }
  let best: { agent: AgentId; n: number; last: Used } | null = null;
  for (const [agent, u] of uses)
    if (best === null || u.n > best.n || (u.n === best.n && later(u.last, best.last)))
      best = { agent, ...u };
  return best?.agent ?? null;
}

/** The agent `create_thread` names. */
export function agentFor({ chosen, setting, threads }: AgentInputs): AgentId {
  if (chosen !== null) return chosen;
  return mostUsed(threads) ?? setting ?? DEFAULT_AGENT;
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

/** The plan of an agent's kind, "Claude" or "ChatGPT"; null for an agent of no kind, which has no plan to name. */
export function knownPlan(agent: AgentId, agents: readonly AgentSummary[] = []): string | null {
  const kind = kindOf(agent, agents);
  return kind === undefined ? null : PLAN[kind];
}

/** The plan a row names for an agent, by its kind: "Claude" or "ChatGPT". An agent of no kind is shown by its id, in
 * English. */
export function planName(agent: AgentId, agents: readonly AgentSummary[] = []): string {
  const kind = kindOf(agent, agents);
  return kind === undefined ? agent : PLAN[kind];
}

/** The plan a matter's rows are measured against (rail-conversations.tsx's `plan`): the plan it uses most, a tie
 * going to the one used most recently (HANDOFF item 9; DESIGN.md now agrees). A row names its plan only where it
 * differs. */
export function matterPlan(
  threads: readonly Used[],
  agents: readonly AgentSummary[] = [],
): string | undefined {
  const agent = mostUsed(threads);
  return agent === null ? undefined : planName(agent, agents);
}

/** The composer's plan choice for a new conversation, from the agents the host lists (R15) that have a kind (a plan
 * to name), each labelled by its kind's plan; none when there is only one to choose. */
export function planOptions(agents: readonly AgentSummary[]): { value: AgentId; label: string }[] {
  const options = agents
    .filter((a) => kindOf(a.id, agents) !== undefined)
    .map((a) => ({ value: a.id, label: planName(a.id, agents) }));
  return options.length > 1 ? options : [];
}
