// UC: what this computer's ProtectionView says beyond a matter's cause: whether a retry is offered, and when it was set
// up, whether a retry brings Windows' administrator prompt, and what the host is doing now. R17 PR-B and PR-E add
// `retry_asks_admin`, `in_progress` and `set_up_at` after #529, so each is read loosely. Outside the room's first load:
// the note's chunk (which hands them to the first run's step) and Settings import it.
import type { ProtectionView } from "../../transport/protocol.ts";
import { notProtectedCause, type Cause } from "./causes.ts";
import { poolOf } from "./detail.ts";

/** The pool's size past which Retry adds no room (R17: 4 more each time, up to 32). */
export const POOL_MAX = 32;

/** Whether a cause offers Try again: the set-up group, and a full pool still under its largest size. */
export const canRetry = (cause: Cause): boolean =>
  cause.group === "setup" || (cause.group === "pool" && poolOf(cause).size < POOL_MAX);

/** R17 PR-E's `set_up_at`, when the view has it: when the set-up was done, in ms. */
export function setUpAt(view: ProtectionView | null | undefined): number | null {
  const value = (view as { set_up_at?: unknown } | null | undefined)?.set_up_at;
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

/** The computer's cause in a view, or null when it is protected (or no view). */
export const computerCause = (view: ProtectionView | null | undefined): Cause | null =>
  view === null || view === undefined ? null : notProtectedCause(view.computer);

/** Whether `retry_protection` brings Windows' administrator prompt, so the "Unknown publisher" confirm comes first
 * (Ruled, 1): R17's `retry_asks_admin` when the view has it, else true when the computer is `needs_set_up` or
 * `set_up_refused`. A full pool's Retry adds room behind the same prompt, so it always asks. */
export function asksAdmin(view: ProtectionView | null | undefined, cause?: Cause | null): boolean {
  if (cause?.group === "pool") return true;
  const field = (view as { retry_asks_admin?: unknown } | null | undefined)?.retry_asks_admin;
  if (typeof field === "boolean") return field;
  const computer = computerCause(view);
  return computer?.name === "needs_set_up" || computer?.name === "set_up_refused";
}

/** R17 PR-B's `in_progress` (set_up, remove, check), read loosely: what the host is doing now, after a reload too. */
export function inProgress(
  view: ProtectionView | null | undefined,
): "set_up" | "remove" | "check" | null {
  const value = (view as { in_progress?: unknown } | null | undefined)?.in_progress;
  return value === "set_up" || value === "remove" || value === "check" ? value : null;
}
