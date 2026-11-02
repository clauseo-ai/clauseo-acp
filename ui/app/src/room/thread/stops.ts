// The one line a step says for how its turn ended. end_turn and at_limit say nothing: a finished answer needs no
// words, and the limit's own row explains a stop at the limit. A failed turn says its failure's sentence.
import type { ThreadKey } from "@clauseo/components/lib/words-thread.ts";
import type { StepEntry, Timestamp } from "../../transport/protocol.ts";

export type StopLine =
  | { kind: "thread"; key: ThreadKey; code?: undefined }
  | { kind: "error"; code: string };

export function stopLine(step: StepEntry): StopLine | null {
  switch (step.stop) {
    case "cancelled":
      return { kind: "thread", key: "You stopped the answer. The answer so far is kept." };
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
