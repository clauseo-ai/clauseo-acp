// The strip's picks over the thread's view (U7, matters/queue and matters/held): the queued messages in the order they
// go, and whether and why the queue is held. Apart from queue-entries.ts's count so that they ride in the strip's chunk
// (queue-strip.tsx), not the room's first load. Each pick returns the same value for as long as it reads the same, so
// a flush that leaves the queue as it was renders nothing; one per mount (useState(queuedPick)).
import type { StreamView } from "../store/view.ts";
import type { StopReason, UserMessageEntry } from "../transport/protocol.ts";
import { userIds, type Held, type Queued } from "./queue-entries.ts";
import { questionText } from "./thread/live.ts";

const STEP = "step:";
const LIMIT = "limit_stop:";

const NONE: readonly Queued[] = [];

const filesOf = (entry: UserMessageEntry) =>
  entry.content.reduce((n, b) => (b.text === undefined ? n + 1 : n), 0);

/** A pick of the queued messages, in the order they go, the same array while each reads the same: one per mount
 * (useState(queuedPick)). The order is the host's (0048, P4b; the reducer's queued_order): the messages put back at the
 * head (`at_head`) first, the latest first, then the rest in their entries' order. */
export function queuedPick(): (v: StreamView | undefined) => readonly Queued[] {
  let last = NONE;
  return (v) => {
    if (v === undefined) return (last = NONE);
    const next: Queued[] = [];
    const ids = userIds(v.ids).filter((id) => v.entries.get(id)?.user_message?.status === "queued");
    const heads = ids.filter((id) => v.entries.get(id)!.user_message!.at_head === true).reverse();
    const order = heads.length === 0 ? ids : [...heads, ...ids.filter((id) => !heads.includes(id))];
    for (const id of order) {
      const entry = v.entries.get(id)!.user_message!;
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

const failedStops: readonly StopReason[] = ["failed", "max_tokens", "max_turn_requests", "refusal"];

/** A pick of why the queue is held, or null while it isn't: the header's `queue_held`, and how the latest turn ended:
 * its step's stop and time, or held with nothing to name when it ended at a limit (a `limit_stop`, no step) or well. The same object while it reads the same: one per mount. */
export function heldPick(): (v: StreamView | undefined) => Held | null {
  let last: Held | null = null;
  return (v) => {
    const head = v?.header?.header;
    if (v === undefined || head?.queue_held !== true) return (last = null);
    // why the host holds it (#537), absent from an older host; a reason this version doesn't know passes as said
    const reason: string | undefined = head.queue_held_reason;
    let next: Held = reason === undefined ? { kind: "held" } : { kind: "held", reason };
    for (let i = v.ids.length - 1; i >= 0; i--) {
      const id = v.ids[i]!;
      // the newest turn's end: a stop at a limit names no stop of the lawyer's, nor an older step's
      if (id.startsWith(LIMIT)) break;
      if (!id.startsWith(STEP)) continue;
      const step = v.entries.get(id)?.step;
      if (step === undefined) continue;
      const why = reason === undefined ? {} : { reason };
      if (step.stop === "cancelled") next = { kind: "stopped", at: step.ts, ...why };
      else if (step.stop === "not_allowed") next = { kind: "not_allowed", at: step.ts, ...why };
      else if (failedStops.includes(step.stop))
        next = {
          kind: "failed",
          at: step.ts,
          ...(step.failure === undefined ? {} : { code: step.failure }),
          ...why,
        };
      break;
    }
    const same =
      last !== null &&
      last.kind === next.kind &&
      last.reason === next.reason &&
      (next.kind === "held" ||
        (last.kind !== "held" && last.at === next.at && last.code === next.code));
    return same ? last : (last = next);
  };
}
