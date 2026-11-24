// Whether the open conversation's message waits for its matter (U7, matters/other, as reworked on 24 Nov): the
// conversations of a matter answer at the same time, so another conversation's answer never holds a message back.
// The host queues a turn (`queued { behind }`) only while an undo, a restore or a relocate holds the matter's writer,
// and that is the only wait the composer shows; the app never raises one on its own. Read from the conversations'
// view (conversations.ts), whose rows carry each thread's status, the held headers' over the list's.
import { useMemo, useSyncExternalStore } from "react";
import type { Conversations, ConversationsView } from "./conversations.ts";

/** Whether the conversation `key`'s message waits for its matter: its status is `queued { behind }`, as the host
 * gives it. `behind` names the thread that holds the writer (its own thread for a restore or a relocate, the undoing
 * thread for an undo), so the composer names none. Another conversation running, starting or waiting on the lawyer
 * is no wait. */
export function heldOf(view: ConversationsView, key: string | null): boolean {
  if (key === null) return false;
  const s = view.rows.find((r) => r.key === key)?.status;
  return typeof s === "object" && s.queued !== undefined;
}

/** Whether the conversation `key`'s message waits for its matter; a change of any other row renders nothing. */
export function useMatterHeld(conversations: Conversations | null, key: string | null): boolean {
  const read = useMemo(
    () => (conversations === null ? () => false : () => heldOf(conversations.view(), key)),
    [conversations, key],
  );
  const subscribe = conversations?.subscribe ?? noSubscribe;
  return useSyncExternalStore(subscribe, read, read);
}

const noSubscribe = () => () => {};
