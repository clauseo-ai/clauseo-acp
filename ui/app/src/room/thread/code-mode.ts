// Whether an answer used Claude's code mode, the one place it is read. Clauseo cannot promise code mode (Anthropic
// decides it per session, by model and by a server gate), so nothing says it is on before an answer has used it. An
// ended answer reads its own step (StepEntry.code_mode, built from its TurnEnded, so a reload keeps it); the running
// answer, which has no step yet, reads the header's code_mode, which the host sets at the turn's first script call
// and clears at the next TurnStarted.
import type { StatusMark } from "@clauseo/components/clauseo/composer.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import type { StreamView } from "../../store/view.ts";
import type { StepEntry } from "../../transport/protocol.ts";
import { turnRunning } from "./live.ts";

/** Whether the answer used code mode: an ended answer by its step; the running one (no step) by the header, only
 * while its turn runs. */
export function codeModeOf(
  answer: { step: StepEntry | undefined },
  view: StreamView | undefined,
): boolean {
  if (answer.step !== undefined) return answer.step.code_mode === true;
  const header = view?.header?.header;
  return turnRunning(header?.status) && header?.code_mode === true;
}

/** The running answer has used code mode: for the status line above the working line. */
export const runningCodeMode = (v: StreamView | undefined): boolean =>
  codeModeOf({ step: undefined }, v);

/** The tag's words, and what assistive technology hears for it. */
export const codeModeTag = (lang: Lang): StatusMark => ({
  label: th(lang, "Code mode"),
  said: th(lang, "Claude used code mode for this answer."),
});
