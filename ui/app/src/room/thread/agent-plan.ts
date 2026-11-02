// The open conversation's plan, for the words that name it (agent.launch_failed: words-errors.ts's launchFailedKey).
// The room gives it (room.tsx, knownPlan's answer for the open conversation's agent) around Work, so the working line,
// the composer and the thread's rows read the same one.
import { createContext, use } from "react";

/** "Claude" or "ChatGPT", null for an agent of no kind, undefined while the room doesn't know (no conversation open,
 * the list loading, or a part mounted outside the room): errorWords then keeps Claude's line. */
export const AgentPlanContext = createContext<string | null | undefined>(undefined);

/** The open conversation's plan, as errorWords takes it. */
export const useAgentPlan = (): string | null | undefined => use(AgentPlanContext);
