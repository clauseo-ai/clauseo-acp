// The open conversation's agent as the words name it (the agent.* codes: words-errors.ts's plannedKey and AgentName).
// The room gives it (room.tsx: knownPlan's answer for the open conversation's agent, else the agent's id) around Work,
// so the working line, the composer and the thread's rows read the same one.
import type { AgentName } from "@clauseo/components/lib/words-errors.ts";
import { createContext, use } from "react";
import { useStreamPart } from "../../store/hooks.ts";
import type { ViewStore } from "../../store/store.ts";
import { streamOf, type StreamView } from "../../store/view.ts";
import type { ThreadId } from "../../transport/protocol.ts";

/** "Claude" or "ChatGPT" for an agent with a kind, else the agent's own name; undefined while the room doesn't know (no
 * conversation open, the list loading, or a part mounted outside the room): errorWords then keeps Claude's line. */
export const AgentPlanContext = createContext<AgentName>(undefined);

/** The open conversation's agent, as errorWords takes it. */
export const useAgentPlan = (): AgentName => use(AgentPlanContext);

const pickAgentName = (v: StreamView | undefined): string | undefined =>
  v?.header?.header?.agent_name || undefined;

/** The open thread's own agent name, from its header (PR #210's agent_name, cleaned by the host); undefined for a draft
 * (no thread), a thread before the field, or an agent with no title. A string pick, so a flush that leaves it changes
 * nothing. */
export function useThreadAgentName(store: ViewStore, thread: ThreadId | null): string | undefined {
  return useStreamPart(store, thread === null ? null : streamOf(thread), pickAgentName);
}
