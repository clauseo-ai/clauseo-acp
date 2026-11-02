// What a conversation's row says it's doing while the lawyer reads another, from its thread's status and H6's marks:
// the rail's ConversationState (rail-words.ts), with the thread it waits behind and the moment it stopped.
import type { ThreadId, Timestamp } from "../transport/protocol.ts";
import type { ConversationRow } from "./conversations.ts";

/** What a row says it's doing while the lawyer reads another (rail-words.ts's ConversationState), with the thread it
 * waits behind and the moment it stopped. */
export type RowWords =
  | { state: "idle" | "working" | "question" | "ready" }
  | { state: "waiting"; behind: ThreadId }
  | { state: "stopped"; at: Timestamp };

/** A row's words, from its status and H6's marks. Starting and running are "Answering."; a pending permission or
 * question is "A question for you"; a turn queued behind another conversation waits for it; an answer finished and
 * unseen is ready, a stop unseen says when. The limit, an agent that exited and an idle stop say nothing here (U12,
 * U10). */
export function rowWords(row: Pick<ConversationRow, "status" | "seen" | "last_stop">): RowWords {
  const s = row.status;
  if (s === "starting" || s === "running") return { state: "working" };
  if (s === "needs_you") return { state: "question" };
  if (typeof s === "object" && s.queued !== undefined)
    return { state: "waiting", behind: s.queued.behind };
  if (s === "idle" && row.seen === false && row.last_stop !== undefined) {
    if (row.last_stop.reason === "cancelled") return { state: "stopped", at: row.last_stop.at };
    if (row.last_stop.reason !== "failed" && row.last_stop.reason !== "at_limit")
      return { state: "ready" };
  }
  return { state: "idle" };
}
