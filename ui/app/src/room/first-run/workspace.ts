// UC: the first run's Safe workspace step (HANDOFF item 5), after the first sign-in and before the first read. It
// stands only when this computer's protection needs its one-time set-up (`needs_set_up`): where the set-up asks an
// administrator (view.ts's asksAdmin) the Windows screen asks, and is itself the confirm (Ruled, 1); where it doesn't,
// the set-up is sent once on entering and the step is its wait. Protected, not known, or the query refused: no step,
// and the first read may start. Can't or a full pool: no step either; the room's line and note say it. Refused,
// failed or Not now is the step's outcome: the room's line says Not protected, and the first read starts.
import type { ProtectionState } from "../../store/protection.ts";
import type * as View from "../protection/view.ts";

/** The view's readers (view.ts), as the note's chunk hands them to the first run, so the two share no chunk. */
export type ViewReaders = Pick<typeof View, "asksAdmin" | "computerCause" | "inProgress">;

/** What the step shows now: "pending" while the view is still being read (the first read waits, nothing shows); the
 * gate's "ask", "waiting" or "wait"; the room's line "off" after an outcome without protection; null for no step. */
export type WorkspaceStep = "pending" | "ask" | "waiting" | "wait" | "off" | null;

/** Whether the step should be entered now: it hasn't been, and the computer needs its set-up. */
export const stepDue = (state: ProtectionState | null, view: ViewReaders | null): boolean =>
  state !== null &&
  view !== null &&
  state.step === null &&
  view.computerCause(state.view)?.name === "needs_set_up";

/** The step for this protection state; null (no step) without the protection wire; "pending" until the readers are
 * in. */
export function workspaceStep(
  state: ProtectionState | null,
  readers: ViewReaders | null,
): WorkspaceStep {
  if (state === null) return null;
  const { view, step } = state;
  if (view === undefined || readers === null) return "pending";
  const { asksAdmin, computerCause, inProgress } = readers;
  const cause = computerCause(view);
  if (view === null || cause === null) return null;
  if (step === null) return cause.name === "needs_set_up" ? "pending" : null;
  if (step === "done") return cause.group === "setup" ? "off" : null;
  const running = state.running === "set_up" || inProgress(view) === "set_up";
  if (!running) return "ask";
  return asksAdmin(view) ? "waiting" : "wait";
}

/** Whether the first read may start: no step, or the step has its outcome. */
export const stepAllowsRead = (step: WorkspaceStep): boolean => step === null || step === "off";
