// The working line while the agent waits to retry its service (host #1335): the thread header's retrying is set by
// the host's reducer from the agent's AgentRetrying and cleared by AgentRetryEnded, the turn's end, a new turn's start
// and a detach. While it is set the line says, in Clauseo's own words, that the plan's servers are busy (UC-1377):
// no countdown, no number, and never the agent's title or details, which the line has no "more" to fold under. A
// reload reads the same header, so a retry under way shows again at once. This file says only whether.
import { useCallback, useSyncExternalStore } from "react";
import type { ViewStore } from "../../store/store.ts";
import type { StreamId } from "../../transport/protocol.ts";

/** Whether the agent of the thread on `stream` waits to retry its service; a boolean, so a new attempt's words, which
 * the line never shows, render nothing. */
export function useRetrying(store: ViewStore | null, stream: StreamId | null): boolean {
  const read = useCallback(
    () =>
      store !== null &&
      stream !== null &&
      store.view(stream)?.header?.header?.retrying !== undefined,
    [store, stream],
  );
  const subscribe = useCallback(
    (on: () => void) => (store === null ? () => {} : store.subscribe(on)),
    [store],
  );
  return useSyncExternalStore(subscribe, read, read);
}
