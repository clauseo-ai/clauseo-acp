// What the open conversation's message waits for (U7, matters/other, as reworked on 24 Nov and in #1302's review):
// the conversations of a matter answer at the same time, so the app never raises a wait on its own; it shows the
// host's `queued { behind }` only, and says only what is true of it:
// - with the host's hold (TurnQueued.hold, folded into the queued status by the reducer: undo, restore or relocate),
//   that hold;
// - with no hold and `behind` naming another conversation (today's host, before #1286), another answer in the matter;
// - with no hold and `behind` its own thread, nothing more than that it waits (the working line).
// Read from the conversations' view (conversations.ts), whose rows carry each thread's status, the held headers' over
// the list's.
import { useMemo, useSyncExternalStore } from "react";
import type { ThreadStatus } from "../transport/protocol.ts";
import type { Conversations, ConversationsView } from "./conversations.ts";

/** The holds the host queues a turn for. */
export type Hold = "undo" | "restore" | "relocate";

/** What the message waits for: a hold the host names, another conversation's answer, or the matter with no reason
 * given. */
export type Wait = { hold: Hold } | { other: true } | { unknown: true };

const HOLDS: readonly string[] = ["undo", "restore", "relocate"];

/** The hold of a queued status. #1286 adds it to the bindings; until then it is read here, and only here, as an
 * optional field of the queued status, so an older host (or log) without it reads as none. */
function holdOf(queued: object): Hold | null {
  const hold: unknown = (queued as { hold?: unknown }).hold;
  return typeof hold === "string" && HOLDS.includes(hold) ? (hold as Hold) : null;
}

/** What the queued status `s` of the thread `own` waits for; null when it isn't queued. */
export function waitOf(s: ThreadStatus | undefined, own: string | null): Wait | null {
  if (typeof s !== "object" || s.queued === undefined) return null;
  const hold = holdOf(s.queued);
  if (hold !== null) return { hold };
  return own !== null && s.queued.behind !== own ? { other: true } : { unknown: true };
}

/** What the conversation `key`'s message waits for, or null when it doesn't wait. */
export function heldOf(view: ConversationsView, key: string | null): Wait | null {
  if (key === null) return null;
  const row = view.rows.find((r) => r.key === key);
  return waitOf(row?.status, row?.thread ?? null);
}

const same = (a: Wait | null, b: Wait | null) => JSON.stringify(a) === JSON.stringify(b);

/** A reader of the wait for `key` that gives the same object for as long as it reads the same. */
function waitReader(conversations: Conversations, key: string | null): () => Wait | null {
  let last: Wait | null = null;
  return () => {
    const next = heldOf(conversations.view(), key);
    if (!same(last, next)) last = next;
    return last;
  };
}

/** What the conversation `key`'s message waits for, the same object while it reads the same: a change of any other
 * row renders nothing. */
export function useMatterHeld(
  conversations: Conversations | null,
  key: string | null,
): Wait | null {
  const read = useMemo(
    () => (conversations === null ? () => null : waitReader(conversations, key)),
    [conversations, key],
  );
  const subscribe = conversations?.subscribe ?? noSubscribe;
  return useSyncExternalStore(subscribe, read, read);
}

const noSubscribe = () => () => {};
