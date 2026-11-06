// The resume cue (R20b): one quiet line while an agent that was stopped for being idle restarts slowly. The thread header's
// resuming_since is set by the reducer (the UserMessage that started the resume) and cleared when the session attaches
// or the status leaves Starting. The line shows only once more than RESUME_CUE_MS have passed since it was set: one
// timer per visible thread, set for the time left, cleared on unmount or when the field changes. Never a poll. The
// words and the strip's line are queue-strip.tsx's; this file says only when.
import { useEffect, useState } from "react";
import type { ViewStore } from "../store/store.ts";
import type { StreamView } from "../store/view.ts";
import { useStreamPart } from "../store/hooks.ts";
import type { StreamId, Timestamp } from "../transport/protocol.ts";

/** How long a resume runs before its cue shows: a restart that finishes sooner shows nothing. */
export const RESUME_CUE_MS = 300;

const pickSince = (v: StreamView | undefined): Timestamp | undefined =>
  v?.header?.header?.resuming_since;

/** Whether the cue shows: the thread is resuming after an idle stop and more than RESUME_CUE_MS have passed since it
 * began. The timer's answer is kept against the time it was for, so a cleared or changed field hides the line at once. */
export function useResumeCue(store: ViewStore, stream: StreamId | null): boolean {
  const since = useStreamPart(store, stream, pickSince);
  const [shownFor, setShownFor] = useState<Timestamp | undefined>(undefined);
  useEffect(() => {
    if (since === undefined) return;
    // more than RESUME_CUE_MS: the time left, plus the millisecond that makes it more; a resume older than that shows
    // on the next turn of the timer
    const left = RESUME_CUE_MS + 1 - (Date.now() - since);
    const timer = setTimeout(() => setShownFor(since), Math.max(left, 0));
    return () => clearTimeout(timer);
  }, [since]);
  return since !== undefined && shownFor === since;
}
