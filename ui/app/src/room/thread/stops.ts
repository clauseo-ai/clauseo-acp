// The one line a step says for how its turn ended. end_turn and at_limit say nothing: a finished answer needs no
// words, and the limit's own row explains a stop at the limit. A failed turn says its failure's sentence, naming the
// item its TurnFailed named (`failure_at`) where the code's words take one. A turn the host stopped because part of
// it tried to run outside the Safe workspace (R17 PR-E's left_safe_workspace, Windows' tier B) says so in a notice.
import type { ThreadKey } from "@clauseo/components/lib/words-thread.ts";
import type { StepEntry, Timestamp } from "../../transport/protocol.ts";

export type StopLine =
  | { kind: "thread"; key: ThreadKey; code?: undefined }
  | { kind: "error"; code: string; at?: string | undefined }
  | { kind: "left_safe_workspace"; code?: undefined };

export function stopLine(step: StepEntry): StopLine | null {
  switch (step.stop) {
    case "cancelled":
      return { kind: "thread", key: "You stopped the answer. The answer so far is kept." };
    case "not_allowed":
      return {
        kind: "thread",
        key: "You didn't allow a request, so the answer stopped. The answer so far is kept.",
      };
    case "max_tokens":
    case "max_turn_requests":
      return { kind: "thread", key: "The answer stopped at its limit, before it finished." };
    case "refusal":
      return { kind: "thread", key: "This question was declined, so there is no answer." };
    case "failed":
      return {
        kind: "error",
        code: step.failure ?? "unknown",
        ...(step.failure_at === undefined ? {} : { at: step.failure_at }),
      };
    case "left_safe_workspace":
      return { kind: "left_safe_workspace" };
    default:
      // end_turn, at_limit, and a stop from a newer Clauseo
      return null;
  }
}

/** Whether the limit's reset is over: at or before `now`. "Resets Saturday" would then read as a limit still in
 * force, so the row says when it reset, in the past tense. */
export const resetPassed = (resets: Timestamp, now: number): boolean => resets <= now;
