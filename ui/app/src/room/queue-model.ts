// What the open conversation waits for while another conversation of its matter holds the matter (U7, matters/other):
// one writer per matter, so a turn sent while another runs waits as `queued { behind }` (the host's call), and before
// sending, the lawyer is told that it will. Read from the conversations' view (conversations.ts), whose rows carry
// each thread's status, the held headers' over the list's, and its title, as the rail shows them.
import { useMemo, useSyncExternalStore } from "react";
import type { ThreadId, ThreadStatus } from "../transport/protocol.ts";
import type { ConversationRow, Conversations, ConversationsView } from "./conversations.ts";

/** The other conversation the open one waits for: its thread, its title (absent while it has none: the rail's "New
 * conversation"), and whether the open one's message has gone (`queued { behind }`, or `starting` on its way there)
 * or not yet. */
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
 *   holds: the reducer gives a sent message `starting` (its user_message) before the host's TurnQueued makes it
 *   `queued { behind }`, so the wait stays in place through that hand-off, naming the other.
 * - Not yet: a message would go (idle, as a draft is; idle_stopped or agent_exited, whose next prompt starts its agent
 *   again: the host refuses neither, crates/host/src/actor/turn/steps.rs's refuse_busy), and another thread holds.
 *
 * Another thread holds when it is running or needs the lawyer, or failing those, starting: the first such in the
 * rows' order (newest activity first). A starting one comes last, as it may itself be about to wait behind the running
 * one; it counts, so a wait raised while the other was only starting doesn't go and come back. */
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
  const holding =
    others.find((r) => holds(r.status, ["running", "needs_you"])) ??
    others.find((r) => holds(r.status, ["starting"]));
  if (holding?.thread == null) return null;
  const wait = { thread: holding.thread, title: titleOf(holding) };
  if (s === "starting") return { ...wait, sent: true };
  return holds(s, ["idle", "idle_stopped", "agent_exited"]) ? { ...wait, sent: false } : null;
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
export function useOtherWait(
  conversations: Conversations | null,
  key: string | null,
): OtherWait | null {
  const read = useMemo(
    () => (conversations === null ? () => null : waitReader(conversations, key)),
    [conversations, key],
  );
  const subscribe = conversations?.subscribe ?? noSubscribe;
  return useSyncExternalStore(subscribe, read, read);
}

const noSubscribe = () => () => {};
