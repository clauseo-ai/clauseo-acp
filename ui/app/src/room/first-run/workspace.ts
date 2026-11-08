// UC: the first run's Safe workspace step (HANDOFF item 5), after the first sign-in and before the
// first read. Design B's installer sets the Safe workspace up, so on a normal install the computer
// is protected and there is no step. The step stands only where the set-up is still due:
// `needs_set_up` (the admin copy is there, after an in-app Remove, say), or R17 PR-E's `blocked {
// admin_copy_missing }` (the installer's prompt declined, a silent per-user install, or the
// uninstaller's `--remove`). For the copy missing, the step is the repair (blocked.ts). For
// `needs_set_up`, where the set-up asks an administrator (view.ts's asksAdmin) the Windows screen
// asks, and is itself the confirm (Ruled, 1); where it doesn't, the set-up is sent once on entering
// and the step is its wait. Protected, not known, or the query refused: no step, and the first read
// may start. Can't, any other block or a full pool: no step either; the room's line and note say
// it. Refused, failed or Not now is the step's outcome: the room's line says Not protected, and the
// first read starts.
import type { ProtectionState } from "../../store/protection.ts";
import type * as Blocked from "../protection/blocked.ts";
import type * as View from "../protection/view.ts";

/** The view's readers (view.ts, blocked.ts), as the note's chunk hands them to the first run, so
 * the two share no chunk. */
export type ViewReaders = Pick<typeof View, "asksAdmin" | "computerCause" | "inProgress"> &
  Pick<typeof Blocked, "blockOf" | "needsInstaller">;

/** What the step shows now: "pending" while the view is still being read (the first read waits,
 * nothing shows); the gate's "repair", "ask", "waiting" or "wait"; the room's line "off" after an
 * outcome without protection; null for no step. */
export type WorkspaceStep = "pending" | "repair" | "ask" | "waiting" | "wait" | "off" | null;

/** Whether the step stands for the computer's state: not set up, or its admin copy missing. */
function entered(view: ProtectionState["view"], readers: ViewReaders): boolean {
  const cause = readers.computerCause(view);
  return cause?.name === "needs_set_up" || readers.needsInstaller(cause);
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
  const { asksAdmin, computerCause, inProgress, needsInstaller } = readers;
  const cause = computerCause(view);
  if (view === null || cause === null) return null;
  const due = entered(view, readers);
  if (step === null) return due ? "pending" : null;
  // a broken set-up (R17 PR-E's M4 repair group) ends the step as the set-up group does
  if (step === "done")
    return cause.group === "setup" || cause.name === "set_up_failed" || due ? "off" : null;
  if (needsInstaller(cause)) return "repair";
  const running = state.running === "set_up" || inProgress(view) === "set_up";
  if (!running) return "ask";
  return asksAdmin(view) ? "waiting" : "wait";
}

/** Whether the first read may start: no step, or the step has its outcome. */
export const stepAllowsRead = (step: WorkspaceStep): boolean => step === null || step === "off";
