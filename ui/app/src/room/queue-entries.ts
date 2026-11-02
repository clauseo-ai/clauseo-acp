// What the thread's queue holds, read from its view (U7, matters/queue and matters/held): how many messages wait, for
// the composer's Queue and the slot over it, and the shapes of the queued messages and the hold that the strip's picks
// give (queue-picks.ts, in the strip's chunk, so the room's first load carries only the count). The pick returns the
// same value for as long as it reads the same, so a flush that leaves the queue as it was renders nothing.
import type { StreamView } from "../store/view.ts";
import type {
  EntryId,
  ErrorCode,
  Timestamp,
  TurnId,
  UserMessageEntry,
} from "../transport/protocol.ts";

/** One queued message: its entry, its turn (what the queue's commands name), its words, and how many file blocks it
 * carries (which an edit keeps). */
export type Queued = {
  readonly id: EntryId;
  readonly turn: TurnId;
  readonly text: string;
  readonly files: number;
  readonly entry: UserMessageEntry;
};

/** Why the queue is held: the lawyer stopped the answer, or it failed, at the step's time; or held with no stop to
 * name (after a restart, or at a limit, U12's), when the strip offers Send next alone. */
export type Held =
  | { readonly kind: "stopped" | "failed"; readonly at: Timestamp; readonly code?: ErrorCode }
  | { readonly kind: "held" };

const USER = "user_message:";

/** The user_message ids of a view's ids, kept per ids array (a new one only as entries come or go). */
const userIdsOf = new WeakMap<readonly EntryId[], readonly EntryId[]>();
export function userIds(ids: readonly EntryId[]): readonly EntryId[] {
  let own = userIdsOf.get(ids);
  if (own === undefined) {
    own = ids.filter((id) => id.startsWith(USER));
    userIdsOf.set(ids, own);
  }
  return own;
}

/** How many messages wait in the queue: for Queue's cap (QUEUE_CAP). */
export function queueCountPick(v: StreamView | undefined): number {
  if (v === undefined) return 0;
  let n = 0;
  for (const id of userIds(v.ids)) if (v.entries.get(id)?.user_message?.status === "queued") n++;
  return n;
}
