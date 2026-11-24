// What the open conversation's message waits for (U7, matters/other, as reworked on 24 Nov and in #1302's review):
// the conversations of a matter answer at the same time, so the app never raises a wait on its own. It shows the
// host's wait only, read from the open thread's header, and says only what is true of it:
// - waiting: the header's status is `queued { behind }` (the message's own status stays sent);
// - why: the header's hold (#1286's HeaderEntry.hold: undo, restore or relocate), that hold; with none and `behind`
//   another conversation (today's host), another answer in the matter; with none and `behind` its own thread, nothing
//   more than that it waits (the working line).
import { useCallback, useSyncExternalStore } from "react";
import type { ViewStore } from "../store/store.ts";
import type { StreamView } from "../store/view.ts";
import type { HeaderEntry, StreamId } from "../transport/protocol.ts";

/** The holds the host queues a turn for: #1286's HeaderEntry.hold. */
export type Hold = "undo" | "restore" | "relocate";

/** The header as #1286 gives it. A stand-in until that binding is on main, when HeaderEntry carries `hold` itself
 * and this becomes HeaderEntry. */
type HeldHeader = HeaderEntry & { hold?: Hold };

/** What the message waits for: a hold the host names, another conversation's answer, or the matter with no reason
 * given. */
export type Wait = Hold | "other" | "unknown";

const HOLDS: readonly string[] = ["undo", "restore", "relocate"];

/** What the open thread's view says its message waits for; null when its header isn't queued. An unknown hold reads
 * as none. */
export function waitOf(v: StreamView | undefined): Wait | null {
  const head = v?.header?.header as HeldHeader | undefined;
  const s = head?.status;
  if (head === undefined || typeof s !== "object" || s.queued === undefined) return null;
  const hold: unknown = head.hold;
  if (typeof hold === "string" && HOLDS.includes(hold)) return hold as Hold;
  return s.queued.behind !== head.thread ? "other" : "unknown";
}

/** What the message of the thread on `stream` waits for; a string, so a change elsewhere in the view renders
 * nothing. */
export function useMatterHeld(store: ViewStore | null, stream: StreamId | null): Wait | null {
  const read = useCallback(
    () => (store === null || stream === null ? null : waitOf(store.view(stream))),
    [store, stream],
  );
  const subscribe = useCallback(
    (on: () => void) => (store === null ? () => {} : store.subscribe(on)),
    [store],
  );
  return useSyncExternalStore(subscribe, read, read);
}
