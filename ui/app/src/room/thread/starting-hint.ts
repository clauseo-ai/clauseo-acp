// The starting hint (UC-873): the working line's one more line once the open thread has sat in Starting for
// STARTING_HINT_MS. A cold start measures about 3.1 s, so the line never shows in the ordinary case. The UI times it: no
// host event is involved. One timer per Starting: any other status clears it, and a later Starting starts a fresh count.
// The words are work-composer.tsx's; this file says only when.
import { useEffect, useState } from "react";
import { useStreamPart } from "../../store/hooks.ts";
import type { ViewStore } from "../../store/store.ts";
import type { StreamView } from "../../store/view.ts";
import type { StreamId } from "../../transport/protocol.ts";

/** How long a thread may sit in Starting before the working line says it takes longer than usual. Well past a cold
 * start (3.1 s), so an ordinary start shows nothing. */
export const STARTING_HINT_MS = 8_000;

const pickStarting = (v: StreamView | undefined): boolean =>
  v?.header?.header?.status === "starting";

/** Whether the open thread's header says Starting. A boolean, so a flush that leaves it changes nothing. */
export function useStarting(store: ViewStore, stream: StreamId | null): boolean {
  return useStreamPart(store, stream, pickStarting);
}

/** Whether the hint shows: `starting` has held for STARTING_HINT_MS. The answer is kept against the run it was for, so
 * a status change hides the line at once, and a later Starting counts from its own start. */
export function useStartingHint(starting: boolean): boolean {
  const [shown, setShown] = useState(false);
  // Starting as the last render had it: a change resets the count during the render, so no stale line shows for a commit
  const [was, setWas] = useState(starting);
  if (was !== starting) {
    setWas(starting);
    setShown(false);
  }
  useEffect(() => {
    if (!starting) return;
    const timer = setTimeout(() => setShown(true), STARTING_HINT_MS);
    return () => clearTimeout(timer);
  }, [starting]);
  return starting && shown;
}
