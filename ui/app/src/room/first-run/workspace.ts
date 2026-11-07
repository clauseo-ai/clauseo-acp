// UC: the first run's Safe workspace step (HANDOFF item 5), after the first sign-in and before the
// first read. Design B's installer sets the Safe workspace up, so on a normal install the computer
// is protected and there is no step. The step stands only where the set-up is still due:
// `needs_set_up`, or R17 PR-E's `blocked { admin_copy_missing }` (the installer's Windows prompt
// declined, or a silent per-user install). Where a set-up with no Windows prompt fixes it, the
// set-up is sent once on entering and the step is its wait. Where the repair is the installer
// (blocked.ts's needsInstaller), the step is the repair, which never raises Windows' prompt.
// Protected, not known, or the query refused: no step, and the first read may start. Can't, any
// other block or a full pool: no step either; the room's line and note say it. Refused, failed or
// Not now is the step's outcome: the room's line says Not protected, and the first read starts.
import type { ProtectionState } from "../../store/protection.ts";
import type * as Blocked from "../protection/blocked.ts";
import type * as View from "../protection/view.ts";

/** The view's readers (view.ts, blocked.ts), as the note's chunk hands them to the first run, so
 * the two share no chunk. */
export type ViewReaders = Pick<typeof View, "computerCause"> &
  Pick<typeof Blocked, "blockOf" | "needsInstaller">;

/** What the step shows now: "pending" while the view is still being read (the first read waits,
 * nothing shows); the gate's "repair" or "wait"; the room's line "off" after an outcome without
 * protection; null for no step. */
export type WorkspaceStep = "pending" | "repair" | "wait" | "off" | null;

/** Whether the computer's state is one the step stands for: not set up yet, or its admin copy
 * missing. */
function entered(view: ProtectionState["view"], readers: ViewReaders): boolean {
  const cause = readers.computerCause(view);
  return cause?.name === "needs_set_up" || readers.blockOf(cause) === "admin_copy_missing";
}

/** Whether the step should be entered now: not yet entered, and the computer's set-up is due. */
export const stepDue = (state: ProtectionState | null, view: ViewReaders | null): boolean =>
  state !== null && view !== null && state.step === null && entered(state.view, view);

/** The step for this protection state; null (no step) without the protection wire; "pending" until the readers are
 * in. */
export function workspaceStep(
  state: ProtectionState | null,
  readers: ViewReaders | null,
): WorkspaceStep {
  if (state === null) return null;
  const { view, step } = state;
  if (view === undefined || readers === null) return "pending";
  const cause = readers.computerCause(view);
  if (view === null || cause === null) return null;
  const due = entered(view, readers);
  if (step === null) return due ? "pending" : null;
  if (step === "done") return cause.group === "setup" || due ? "off" : null;
  return readers.needsInstaller(view, cause) ? "repair" : "wait";
}

/** Whether the first read may start: no step, or the step has its outcome. */
export const stepAllowsRead = (step: WorkspaceStep): boolean => step === null || step === "off";
