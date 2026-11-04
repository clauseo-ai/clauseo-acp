// One visited matter's conversations as conversations.ts keeps them, and a fresh one. Apart from conversations.ts to
// keep that file under the size limit.
import type { AgentId, ErrorCode, MatterId, ThreadId, Timestamp } from "../transport/protocol.ts";
import type { ConversationsView, Known } from "./conversations-view.ts";

/** Each visited matter's conversations, kept while the room lives. */
export type MatterState = {
  matter: MatterId;
  threads: Map<ThreadId, Known>;
  /** Threads known to the room by a key other than their id. */
  keys: Map<ThreadId, string>;
  /** The draft: its key, whether its row shows (New conversation was pressed), and the plan chosen for it. */
  draft: { key: string; shown: boolean; chosen: AgentId | null } | null;
  /** The conversation open, by key; null before the matter is first opened. */
  open: string | null;
  /** The key that stands for what opens before the list says what that is: the thread to reopen, or a draft. */
  pending: string | null;
  phase: ConversationsView["phase"];
  refused: ErrorCode | null;
  /** The refused list's error, for a first send that can't go without it. */
  refusal: unknown;
  /** The list read in flight, for a first send while it loads. */
  listing: Promise<void> | null;
  ticket: number;
  /** Threads given a rename by this room: one each. */
  renamed: Set<ThreadId>;
  /** H13: threads this room retried, with the thread and the time of the question they came from. */
  retried: Map<ThreadId, { from: ThreadId; at: Timestamp }>;
  /** Threads this room made (create_thread, retry_from): a list read that started before one was made still keeps it. */
  made: Set<ThreadId>;
  /** Threads this room made whose first question the host hasn't accepted yet (send_prompt refused): left with
   * nothing typed, they are archived, so no empty thread stays on the list. */
  unsent: Set<ThreadId>;
  /** A thread opened with its summary (Find's archived result) that the host's list leaves out: kept among the
   * threads, over the list's reads, while it is the one open. */
  kept: Known | null;
};

/** A matter visited for the first time: nothing open yet, its list on its way. */
export const freshState = (matter: MatterId): MatterState => ({
  matter,
  threads: new Map(),
  keys: new Map(),
  draft: null,
  open: null,
  pending: null,
  phase: "loading",
  refused: null,
  refusal: null,
  listing: null,
  ticket: 0,
  renamed: new Set(),
  retried: new Map(),
  made: new Set(),
  unsent: new Set(),
  kept: null,
});

/** A list just read or heard (`threads`) keeps the thread opened with its summary, while it is still the one open; once
 * another opens it is let go. */
export function keepKept(m: MatterState, threads: Map<ThreadId, Known>): void {
  const kept = m.kept;
  if (kept === null) return;
  if (m.open !== (m.keys.get(kept.thread) ?? kept.thread)) m.kept = null;
  else if (!threads.has(kept.thread)) threads.set(kept.thread, kept);
}
