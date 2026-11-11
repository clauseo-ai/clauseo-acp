// What the host's step_undone marker on a conversation's stream says of one turn's undo: for
// Versions' line after it (versions/reading.ts) and the thread's change note
// (thread/stop-rows.tsx). Small and alone, so the thread's chunk doesn't carry Versions'.
import type { StreamView } from "../store/view.ts";
import type { SkippedFile, TurnId } from "../transport/protocol.ts";

/**  The step_undone marker's body for the undo of `turn`, the newest on the stream's window; null
 * until it comes. */
export function stepUndoneOf(
  view: StreamView | undefined,
  turn: TurnId,
): { restored: readonly string[]; unversioned: readonly SkippedFile[] } | null {
  let found: { restored: readonly string[]; unversioned: readonly SkippedFile[] } | null = null;
  for (const id of view?.ids ?? []) {
    const undone = view?.entries.get(id)?.marker?.kind.step_undone;
    if (undone?.turn === turn)
      found = { restored: undone.restored, unversioned: undone.unversioned ?? NONE };
  }
  return found;
}

const NONE: readonly SkippedFile[] = [];

/**  The files of `turn`'s undo that stay as they are because Clauseo had no earlier copy of them;
 * the same array while the marker is unchanged, as a store hook's pick needs. Empty when none, or
 * until the marker comes. */
export function unversionedOf(view: StreamView | undefined, turn: TurnId): readonly SkippedFile[] {
  let found = NONE;
  for (const id of view?.ids ?? []) {
    const undone = view?.entries.get(id)?.marker?.kind.step_undone;
    if (undone?.turn === turn) found = undone.unversioned ?? NONE;
  }
  return found;
}
