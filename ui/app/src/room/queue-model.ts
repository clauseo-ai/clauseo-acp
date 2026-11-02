// What the open conversation waits for while another conversation of its matter holds the matter (U7, matters/other):
// one writer per matter, so a turn sent while another runs waits as `queued { behind }` (the host's call), and before
// sending, the lawyer is told that it will. Read from the conversations' view (conversations.ts), whose rows carry
// each thread's status, the held headers' over the list's, and its title, as the rail shows them.
import { useMemo, useSyncExternalStore } from "react";
import type { ThreadId, ThreadStatus } from "../transport/protocol.ts";
import type { ConversationRow, Conversations, ConversationsView } from "./conversations.ts";

/** The other conversation the open one waits for: its thread, its title (absent while it has none: the rail's "New
 * conversation"), and whether the open one's message has gone (`queued { behind }`) or not yet. */
export type OtherWait = {
  thread: ThreadId;
  title: string | undefined;
  sent: boolean;
};

const holds = (s: ThreadStatus, which: readonly string[]) =>
  typeof s === "string" && which.includes(s);

/** The wait over the composer of the conversation `key`, or null for none.
 *
 * - Sent: its status is `queued { behind }`: the thread the host names. Or it is `starting` while another thread
 *   runs or needs the lawyer: the reducer gives a sent message `starting` (its user_message) before the host's
 *   TurnQueued makes it `queued { behind }`, so the wait stays in place through that hand-off, naming the other.
 * - Not yet: it is idle (a draft is) or idle_stopped, and another thread of the matter is running or needs the lawyer, or failing
 *   those, starting: the first such in the rows' order (newest activity first). A starting one comes last, as it may
 *   itself be about to wait behind the running one. */
export function otherWaitOf(view: ConversationsView, key: string | null): OtherWait | null {
  if (key === null) return null;
  const rows = view.rows;
  const own = rows.find((r) => r.key === key);
  if (own === undefined) return null;
  const s = own.status;
  const titleOf = (row: ConversationRow | undefined) => row?.title;
  if (typeof s === "object" && s.queued !== undefined) {
    const behind = s.queued.behind;
    return { thread: behind, title: titleOf(rows.find((r) => r.thread === behind)), sent: true };
  }
  const others = rows.filter((r) => r.key !== key && r.thread !== null && r.thread !== own.thread);
  const holding = others.find((r) => holds(r.status, ["running", "needs_you"]));
  if (s === "starting")
    return holding?.thread == null
      ? null
      : { thread: holding.thread, title: titleOf(holding), sent: true };
  // idle, or stopped for being idle, which its next prompt starts again: a message would go
  if (s !== "idle" && s !== "idle_stopped") return null;
  const other = holding ?? others.find((r) => holds(r.status, ["starting"]));
  if (other?.thread == null) return null;
  return { thread: other.thread, title: titleOf(other), sent: false };
}

const same = (a: OtherWait | null, b: OtherWait | null) =>
  a === b ||
  (a !== null && b !== null && a.thread === b.thread && a.title === b.title && a.sent === b.sent);

/** A reader of the wait for `key` that gives the same object for as long as it reads the same. */
function waitReader(conversations: Conversations, key: string | null): () => OtherWait | null {
  let last: OtherWait | null = null;
  return () => {
    const next = otherWaitOf(conversations.view(), key);
    if (!same(last, next)) last = next;
    return last;
  };
}

/** The wait for the conversation `key`, the same object for as long as it reads the same: a change of any other row,
 * or of the other conversation beyond its holding and its title, renders nothing. */
export function useOtherWait(conversations: Conversations, key: string | null): OtherWait | null {
  const read = useMemo(() => waitReader(conversations, key), [conversations, key]);
  return useSyncExternalStore(conversations.subscribe, read, read);
}
