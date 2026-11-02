// The agent a new conversation runs, fixed at `create_thread` by its first message (HANDOFF item 9), and the plan name a
// row shows for it. The order, first that is given (Rohan, 2026-11-02, HANDOFF item 9): the lawyer's choice in the
// composer (R15's ListAgents gives the options); the plan used most in the matter, a tie going to the one used most
// recently; in a matter with no conversation, the "Start new conversations with" setting (H7's default-agent key, which
// U11 sets; not on the wire yet, so null today); else the fallback (DEFAULT_AGENT, or a listed agent when the host
// doesn't list that one). A plan is counted by kind, not by agent id: `claude` and `claude-acp` are both Claude, so
// they count together, and the agent the matter runs is the one of the winning kind used most recently. Agent ids are
// the settings' keys (`agents.<id>`), which R15 renamed from `claude` to `claude-acp`, so nothing here keys on an id
// but DEFAULT_AGENT and the canonical ids, and the plan names go by the agent's kind (R15's `AgentSummary`), read off
// the id's start only for an agent the list doesn't hold.
import type { AgentId, AgentKind, ThreadSummary, Timestamp } from "../transport/protocol.ts";
import type { AgentSummary } from "./host-next.ts";

/** The agent when nothing else says: the settings' `agents.claude-acp` (ui/e2e/app-windows/scratch.ts's AGENT_ID, and
 * the id R15 named). */
export const DEFAULT_AGENT: AgentId = "claude-acp";

/** What decides a new conversation's agent, strongest first. */
export type AgentInputs = {
  /** The lawyer's choice in the composer for this conversation, if they made one. */
  chosen: AgentId | null;
  /** The "Start new conversations with" setting (H7), if the host gives it. */
  setting: AgentId | null;
  /** The matter's conversations, archived ones left out. */
  threads: readonly Used[];
  /** The agents the host lists (R15's ListAgents), for their kinds; empty before the list arrives. */
  agents: readonly AgentSummary[];
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

/** The kinds in the order the composer lists their plans: Claude first. */
const KINDS: readonly AgentKind[] = ["claude", "codex"];

/** Each kind's plan, as the rows and the composer's choice name it. */
const PLAN: Record<AgentKind, string> = { claude: "Claude", codex: "ChatGPT" };

/** Each kind's canonical agent id: the one the plan option prefers when the host lists it. */
const CANONICAL: Record<AgentKind, AgentId> = { claude: DEFAULT_AGENT, codex: "codex-acp" };

/** An agent's kind: the one the host lists for it (R15's ListAgents), else read off its id (Claude Code's begin
 * `claude`, Codex's `codex`), for an agent the list doesn't hold or lists with no kind (#154 gives a kind to
 * `claude-acp` and `codex-acp` only, so an older settings file's `claude` lists without one); none for any other id. */
export function kindOf(agent: AgentId, agents: readonly AgentSummary[]): AgentKind | undefined {
  const listed = agents.find((a) => a.id === agent);
  if (listed?.kind !== undefined) return listed.kind;
  if (agent.startsWith("claude")) return "claude";
  if (agent.startsWith("codex")) return "codex";
  return undefined;
}

/** The group an agent's uses count in: its kind, so that `claude` and `claude-acp` count together. An agent of no kind
 * is a group of its own, keyed by its id, so that it never pools with another agent. */
function groupOf(agent: AgentId, agents: readonly AgentSummary[]): string {
  return kindOf(agent, agents) ?? `agent:${agent}`;
}

/** The matter's winning group, counted by kind (see the top of this file), a tie going to the group used most recently:
 * its kind (none for an agent of no kind) and its conversations; null with no conversation. Every conversation counts,
 * listed or not: a matter whose conversations ran `claude` before R15's rename still uses the Claude plan. */
function winner(
  threads: readonly Used[],
  agents: readonly AgentSummary[],
): { kind: AgentKind | undefined; members: Used[]; last: Used } | null {
  const uses = new Map<string, { members: Used[]; last: Used }>();
  for (const t of threads) {
    const group = groupOf(t.agent, agents);
    const u = uses.get(group);
    if (u === undefined) uses.set(group, { members: [t], last: t });
    else {
      u.members.push(t);
      if (later(t, u.last)) u.last = t;
    }
  }
  let best: { members: Used[]; last: Used } | null = null;
  for (const u of uses.values())
    if (
      best === null ||
      u.members.length > best.members.length ||
      (u.members.length === best.members.length && later(u.last, best.last))
    )
      best = u;
  return best === null ? null : { ...best, kind: kindOf(best.last.agent, agents) };
}

/** The agent a new conversation runs by the matter's use: the winning group's most recently used agent. Where the host
 * lists agents, only a listed one: the group's most recent listed agent, else its kind's option (choiceOf: the
 * canonical or first listed id), else null, so a conversation never starts on an id the settings no longer hold (a
 * pre-rename `claude` beside today's `claude-acp`). Null with no conversation. */
export function mostUsed(
  threads: readonly Used[],
  agents: readonly AgentSummary[] = [],
): AgentId | null {
  const won = winner(threads, agents);
  if (won === null) return null;
  if (agents.length === 0) return won.last.agent;
  let listed: Used | undefined;
  for (const t of won.members)
    if (isListed(t.agent, agents) && (listed === undefined || later(t, listed))) listed = t;
  if (listed !== undefined) return listed.agent;
  return won.kind === undefined ? null : (choiceOf(won.kind, agents, []) ?? null);
}

/** Whether the host lists `agent`. */
const isListed = (agent: AgentId, agents: readonly AgentSummary[]): boolean =>
  agents.some((a) => a.id === agent);

/** The agent a conversation runs when nothing else names one: DEFAULT_AGENT, unless the host lists agents and does not
 * list that one. Then it is the Claude plan's agent, else the first listed agent with a kind, else the first listed,
 * so that a conversation never starts on an agent the host does not run. */
function fallbackOf(agents: readonly AgentSummary[]): AgentId {
  if (agents.length === 0 || agents.some((a) => a.id === DEFAULT_AGENT)) return DEFAULT_AGENT;
  return (
    choiceOf("claude", agents, []) ??
    agents.find((a) => kindOf(a.id, agents) !== undefined)?.id ??
    agents[0]?.id ??
    DEFAULT_AGENT
  );
}

/** The agent `create_thread` names. */
export function agentFor({ chosen, setting, threads, agents }: AgentInputs): AgentId {
  if (chosen !== null) return chosen;
  return mostUsed(threads, agents) ?? setting ?? fallbackOf(agents);
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
  const won = winner(threads, agents);
  return won === null ? undefined : planName(won.last.agent, agents);
}

/** The agent a plan option stands for: the listed id of that kind the matter used most recently (from `threads`); else
 * the kind's canonical id if the host lists it; else the first listed id of the kind. Undefined when the host lists no
 * agent of the kind. */
function choiceOf(
  kind: AgentKind,
  agents: readonly AgentSummary[],
  threads: readonly Used[],
): AgentId | undefined {
  const listed = agents.filter((a) => kindOf(a.id, agents) === kind).map((a) => a.id);
  if (listed.length === 0) return undefined;
  let used: Used | undefined;
  for (const t of threads)
    if (listed.includes(t.agent) && (used === undefined || later(t, used))) used = t;
  if (used !== undefined) return used.agent;
  return listed.includes(CANONICAL[kind]) ? CANONICAL[kind] : listed[0];
}

/** The composer's plan choice for a new conversation: one option per kind the host lists (R15), labelled by the kind's
 * plan, Claude first. Each option's value is chosen by `choiceOf`. None when there is only one plan to choose. */
export function planOptions(
  agents: readonly AgentSummary[],
  threads: readonly Used[] = [],
): { value: AgentId; label: string }[] {
  const options = KINDS.flatMap((kind) => {
    const value = choiceOf(kind, agents, threads);
    return value === undefined ? [] : [{ value, label: PLAN[kind] }];
  });
  return options.length > 1 ? options : [];
}
