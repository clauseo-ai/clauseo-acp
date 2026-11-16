// The order of the open matter's conversations on the rail, pure but for the order it holds: what needs the lawyer
// first, then what is answering, then the rest by their latest activity. In rank order:
// - the draft, where shown, stays at the top, where New conversation put it;
// - Needs you, and nothing else (D-037): a conversation waiting on the lawyer's hand (a permission or a question), then
//   one whose answer finished unseen (Answer ready); a failure unseen says so on its row but stands with the rest;
// - Working: answering now, or queued behind another conversation's answer;
// - the rest. Within a rank the list's own order holds: the newest activity first (conversations.ts).
// A snoozed or done conversation needs nothing until it wakes or is taken back, as the matters' triage has it. While the
// pointer or the focus is on the rail the rows keep their places (`hold`), new ones joining at the foot, so nothing
// moves under a press; they settle when it leaves, as the matters do (rail-triage.ts).
import type { Timestamp } from "../transport/protocol.ts";
import type { ConversationRow } from "./conversations.ts";
import { needOf, snoozed, type Need } from "./triage.ts";

const NEED_RANK: Record<Need, number> = { hand: 1, ready: 2, failed: 5 };

/** A row's rank: 0 the draft, 1 and 2 Needs you (the hand, an answer ready), 4 Working, 5 the rest (a failure unseen
 * included). */
export function conversationRank(row: ConversationRow, now: Timestamp): number {
  if (row.thread === null) return 0;
  if (row.done === true || snoozed(row, now)) return 5;
  const need = needOf(row);
  if (need === "hand" || need === "ready") return NEED_RANK[need];
  const s = row.status;
  if (s === "starting" || s === "running" || (typeof s === "object" && s.queued !== undefined))
    return 4;
  return 5;
}

/** The rows by rank, the list's order kept within each. */
export function orderConversations(
  rows: readonly ConversationRow[],
  now: Timestamp,
): ConversationRow[] {
  return rows
    .map((row, i) => ({ row, i, rank: conversationRank(row, now) }))
    .sort((a, b) => a.rank - b.rank || a.i - b.i)
    .map((r) => r.row);
}

/** The order over time: fresh at rest, held while the pointer or the focus is on the rail. */
export class ConversationOrder {
  #last: readonly string[] = [];

  of(rows: readonly ConversationRow[], now: Timestamp, hold: boolean): ConversationRow[] {
    const fresh = orderConversations(rows, now);
    if (!hold) {
      this.#last = fresh.map((r) => r.key);
      return fresh;
    }
    const place = new Map(this.#last.map((key, i) => [key, i]));
    const kept = fresh
      .filter((r) => place.has(r.key))
      .sort((a, b) => (place.get(a.key) ?? 0) - (place.get(b.key) ?? 0));
    // the draft keeps the top even while held: New conversation put it there under the pointer
    const draft = fresh.filter((r) => r.thread === null && !place.has(r.key));
    const joined = fresh.filter((r) => r.thread !== null && !place.has(r.key));
    const held = [...draft, ...kept, ...joined];
    this.#last = held.map((r) => r.key);
    return held;
  }
}
