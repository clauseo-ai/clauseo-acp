// The one line a step says for how its turn ended. end_turn and at_limit say nothing: a finished answer needs no
// words, and the limit's own row explains a stop at the limit. A failed turn says its failure's sentence.
import type { AgentName } from "@clauseo/components/lib/words-errors.ts";
import type { ThreadKey } from "@clauseo/components/lib/words-thread.ts";
import type { StepEntry, Timestamp } from "../../transport/protocol.ts";

export type StopLine =
  | { kind: "thread"; key: ThreadKey; slots?: { name: string }; code?: undefined }
  | { kind: "error"; code: string };

/** The agent as the error words name it (words-errors.ts's plannedKey): Claude, ChatGPT for Codex, else its own name;
 * null where the room doesn't know it or it has no name. */
function agentNameOf(agent: AgentName): string | null {
  if (agent?.kind === "claude") return "Claude";
  if (agent?.kind === "codex") return "ChatGPT";
  const name = agent?.name?.trim() ?? "";
  return name === "" ? null : name;
}

/** `agent`: the open conversation's agent (useAgentPlan), named by a stop Clauseo made for the agent's request. */
export function stopLine(step: StepEntry, agent?: AgentName): StopLine | null {
  switch (step.stop) {
    case "cancelled":
      return { kind: "thread", key: "You stopped the answer. The answer so far is kept." };
    case "not_allowed":
      return {
        kind: "thread",
        key: "You didn't allow a request, so the answer stopped. The answer so far is kept.",
      };
    case "outside_matter": {
      const name = agentNameOf(agent);
      return name === null
        ? {
            kind: "thread",
            key: "Clauseo stopped the answer: the AI tried to open files outside this matter. The answer so far is kept.",
          }
        : {
            kind: "thread",
            key: "Clauseo stopped the answer: {name} tried to open files outside this matter. The answer so far is kept.",
            slots: { name },
          };
    }
    case "max_tokens":
    case "max_turn_requests":
      return { kind: "thread", key: "The answer stopped at its limit, before it finished." };
    case "refusal":
      return { kind: "thread", key: "This question was declined, so there is no answer." };
    case "failed":
      return { kind: "error", code: step.failure ?? "unknown" };
    default:
      // end_turn, at_limit, and a stop from a newer Clauseo
      return null;
  }
}

/** Whether the limit's reset is over: at or before `now`. "Resets Saturday" would then read as a limit still in
 * force, so the row says when it reset, in the past tense. */
export const resetPassed = (resets: Timestamp, now: number): boolean => resets <= now;
