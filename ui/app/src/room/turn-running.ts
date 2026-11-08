// R23: whether a turn runs in any matter now, for the update notice's After this answer: every matter's threads
// (all-threads.ts, the app feed's lists), read as the host's after_turn reads them.
import { useCallback, useSyncExternalStore } from "react";
import type { ThreadStatus } from "../transport/protocol.ts";
import type { AllThreads, AllThreadsView } from "./all-threads.ts";

/** Whether a thread's status is a turn under way: starting, running, waiting on the lawyer, or queued behind another
 * thread's. The host's after_turn waits for each of these. */
export const turnUnderWay = (status: ThreadStatus): boolean =>
  status === "starting" ||
  status === "running" ||
  status === "needs_you" ||
  (typeof status === "object" && status.queued !== undefined);

/** Whether any matter's thread has a turn under way, from every matter's threads (all-threads.ts). */
export const anyTurn = (view: AllThreadsView): boolean => {
  for (const threads of view.lists.values())
    if (threads.some((t) => turnUnderWay(t.status))) return true;
  return false;
};

const nothing = () => () => {};

/** Whether a turn runs in any matter now; false without the threads' store. */
export function useTurnRunning(threads: AllThreads | null): boolean {
  const read = useCallback(() => (threads === null ? false : anyTurn(threads.view())), [threads]);
  return useSyncExternalStore(threads?.subscribe ?? nothing, read, read);
}
