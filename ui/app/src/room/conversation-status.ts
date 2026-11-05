// What a conversation's row says it's doing while the lawyer reads another, from its thread's status and H6's marks:
// the rail's ConversationState (rail-words.ts), with the thread it waits behind and the moment it stopped.
import type { ThreadId, Timestamp } from "../transport/protocol.ts";
import type { ConversationRow } from "./conversations.ts";
import type { MatterStatus } from "./host-next.ts";

/** What a row says it's doing while the lawyer reads another (rail-words.ts's ConversationState), with the thread it
 * waits behind and the moment it stopped. */
export type RowWords =
  | { state: "idle" | "working" | "question" | "permission" | "ready" }
  | { state: "waiting"; behind: ThreadId }
  | { state: "stopped" | "failed"; at: Timestamp };

/** A row's words, from its status and H6's marks. Starting and running are "Answering."; a thread that waits on the
 * lawyer takes its matter's kind (`matter`, H5's status), since its own summary can't tell them apart: a permission
 * where the matter needs one, else a question, "A question for you"; a turn queued behind another conversation waits for it; an answer finished and
 * unseen is ready, a stop unseen says when, and so does a failure unseen (RO-23). The limit, an agent that exited and an
 * idle stop say nothing else here (U12, U10). */
export function rowWords(
  row: Pick<ConversationRow, "status" | "seen" | "last_stop">,
  matter?: MatterStatus,
): RowWords {
  const s = row.status;
  if (s === "starting" || s === "running") return { state: "working" };
  if (s === "needs_you")
    return { state: matter === "needs_permission" ? "permission" : "question" };
  if (typeof s === "object" && s.queued !== undefined)
    return { state: "waiting", behind: s.queued.behind };
  const resting = s === "idle" || s === "agent_exited" || s === "idle_stopped";
  if (resting && row.seen === false && row.last_stop?.reason === "failed")
    return { state: "failed", at: row.last_stop.at };
  if (s === "idle" && row.seen === false && row.last_stop !== undefined) {
    if (row.last_stop.reason === "cancelled" || row.last_stop.reason === "not_allowed")
      return { state: "stopped", at: row.last_stop.at };
    if (row.last_stop.reason !== "failed" && row.last_stop.reason !== "at_limit")
      return { state: "ready" };
  }
  return { state: "idle" };
}
