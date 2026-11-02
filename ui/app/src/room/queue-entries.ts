// What the thread's queue holds, read from its view (U7, matters/queue and matters/held): its queued messages in the
// order they go, how many, and whether and why it is held. Each pick returns the same value for as long as it reads
// the same, so a flush that leaves the queue as it was renders nothing (the strip, the composer's Queue).
import type { StreamView } from "../store/view.ts";
import type {
  EntryId,
  ErrorCode,
  StopReason,
  Timestamp,
  TurnId,
  UserMessageEntry,
} from "../transport/protocol.ts";
import { questionText } from "./thread/live.ts";

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
const STEP = "step:";

/** The user_message ids of a view's ids, kept per ids array (a new one only as entries come or go). */
const userIdsOf = new WeakMap<readonly EntryId[], readonly EntryId[]>();
function userIds(ids: readonly EntryId[]): readonly EntryId[] {
  let own = userIdsOf.get(ids);
  if (own === undefined) {
    own = ids.filter((id) => id.startsWith(USER));
    userIdsOf.set(ids, own);
  }
  return own;
}

const NONE: readonly Queued[] = [];

const filesOf = (entry: UserMessageEntry) =>
  entry.content.reduce((n, b) => (b.text === undefined ? n + 1 : n), 0);

/** A pick of the queued messages, in the order they go (their entries' order), the same array while each reads the
 * same: one per mount (useState(queuedPick)). */
export function queuedPick(): (v: StreamView | undefined) => readonly Queued[] {
  let last = NONE;
  return (v) => {
    if (v === undefined) return (last = NONE);
    const next: Queued[] = [];
    for (const id of userIds(v.ids)) {
      const entry = v.entries.get(id)?.user_message;
      if (entry?.status !== "queued") continue;
      const prev = last[next.length];
      const text = questionText(id, entry, v.texts);
      next.push(
        prev !== undefined && prev.id === id && prev.entry === entry && prev.text === text
          ? prev
          : { id, turn: entry.turn, text, files: filesOf(entry), entry },
      );
    }
    if (next.length === last.length && next.every((q, i) => q === last[i])) return last;
    return (last = next.length === 0 ? NONE : next);
  };
}

/** How many messages wait in the queue: for Queue's cap (QUEUE_CAP). */
export function queueCountPick(v: StreamView | undefined): number {
  if (v === undefined) return 0;
  let n = 0;
  for (const id of userIds(v.ids)) if (v.entries.get(id)?.user_message?.status === "queued") n++;
  return n;
}

const failedStops: readonly StopReason[] = ["failed", "max_tokens", "max_turn_requests", "refusal"];

/** A pick of why the queue is held, or null while it isn't: the header's `queue_held`, and the latest step's stop and
 * time. The same object while it reads the same: one per mount. */
export function heldPick(): (v: StreamView | undefined) => Held | null {
  let last: Held | null = null;
  return (v) => {
    if (v?.header?.header?.queue_held !== true) return (last = null);
    let next: Held = { kind: "held" };
    for (let i = v.ids.length - 1; i >= 0; i--) {
      const id = v.ids[i]!;
      if (!id.startsWith(STEP)) continue;
      const step = v.entries.get(id)?.step;
      if (step === undefined) continue;
      if (step.stop === "cancelled") next = { kind: "stopped", at: step.ts };
      else if (failedStops.includes(step.stop))
        next = {
          kind: "failed",
          at: step.ts,
          ...(step.failure === undefined ? {} : { code: step.failure }),
        };
      break;
    }
    const same =
      last !== null &&
      last.kind === next.kind &&
      (next.kind === "held" ||
        (last.kind !== "held" && last.at === next.at && last.code === next.code));
    return same ? last : (last = next);
  };
}
