// What the host's step_undone marker on a conversation's stream says of one turn's undo, for the
// thread's change note (thread/stop-rows.tsx). Versions reads the marker itself
// (versions/reading.ts): a module both imported would be a chunk of its own that the thread's lazy
// list imports, which the release build allows only for a chunk that bare chunks alone import.
import type { StreamView } from "../store/view.ts";
import type { SkippedFile, TurnId } from "../transport/protocol.ts";

const NONE: readonly SkippedFile[] = [];

/**  The files `turn`'s undo put back, as the host's marker lists them; null until it comes. The
 * same array while the marker is unchanged. */
export function undoneRestoredOf(
  view: StreamView | undefined,
  turn: TurnId,
): readonly string[] | null {
  let found: readonly string[] | null = null;
  for (const id of view?.ids ?? []) {
    const undone = view?.entries.get(id)?.marker?.kind.step_undone;
    if (undone?.turn === turn) found = undone.restored;
  }
  return found;
}

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
