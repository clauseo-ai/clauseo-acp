// The time of a question or a step in the thread, for the sections' names and the answer's meta line.
import { useMemo } from "react";
import { useStreamPart, useViewStore } from "../../store/hooks.ts";
import type { StreamView } from "../../store/view.ts";
import type { EntryId, StreamId, Timestamp, ViewEntry } from "../../transport/protocol.ts";

const timeIn = (e: ViewEntry | undefined): Timestamp | null =>
  e?.user_message?.ts ?? e?.step?.ts ?? null;

/** The time of a question or a step, by id, from an older page or the window; null with no id or while not in. A
 * number, so a flush that leaves it as it was renders nothing. */
export function useTimeOf(
  id: EntryId | undefined,
  stream: StreamId,
  paged: ReadonlyMap<EntryId, ViewEntry>,
): Timestamp | null {
  const pick = useMemo(
    () => (v: StreamView | undefined) => (id === undefined ? null : timeIn(v?.entries.get(id))),
    [id],
  );
  const live = useStreamPart(useViewStore(), stream, pick);
  const page = id === undefined ? undefined : paged.get(id);
  return page === undefined ? live : timeIn(page);
}

/** When an answer started, as far as the view can say it honestly: its question's time, when the question went
 * straight to an agent that was free (the turn before had ended by then, and the turn doesn't wait in a queue now);
 * null otherwise, since the view keeps no time for a turn's start, and a question that waited would claim the answer
 * began earlier than it did. */
export function startOf(
  asked: Timestamp | null,
  before: Timestamp | null,
  queued: boolean,
): Timestamp | null {
  // a question with no time (0, or none) gives none
  if (asked === null || asked <= 0 || queued) return null;
  return before !== null && before > asked ? null : asked;
}
