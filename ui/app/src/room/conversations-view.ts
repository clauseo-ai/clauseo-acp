// What the conversations (conversations.ts) show: a thread as a row knows it, one row, and the view the rail and the
// work read. Apart from conversations.ts to keep that file under the size limit.
import type {
  AgentId,
  ErrorCode,
  MatterId,
  ThreadId,
  ThreadStatus,
  Timestamp,
} from "../transport/protocol.ts";
import type { AgentSummary, ThreadInfo } from "./host-next.ts";

/** A thread as a row knows it: the list's summary, with H6's marks where given. */
export type Known = Omit<ThreadInfo, "seen"> & { seen?: boolean };

/** One row: a thread, or the draft (no thread, no agent yet). */
export type ConversationRow = {
  key: string;
  thread: ThreadId | null;
  /** Its title; absent until it has one, when the row says "New conversation". */
  title?: string;
  /** Its agent; null for a draft, whose agent is fixed by its first send. */
  agent: AgentId | null;
  status: ThreadStatus;
  /** H6: false while its last answer waits to be seen; absent without H6. */
  seen?: boolean;
  last_stop?: Known["last_stop"];
};

export type ConversationsView = {
  matter: MatterId | null;
  /** loading until the matter's list is in; failed when it couldn't be read. */
  phase: "loading" | "ready" | "failed";
  /** The code of the refused list, until a read succeeds. */
  refused: ErrorCode | null;
  /** The draft first, if shown, then the threads, newest activity first; archived threads left out. */
  rows: readonly ConversationRow[];
  /** The key of the conversation the work shows; null with no matter open. */
  open: string | null;
  /** Its thread: null for a draft, or while the list loads. */
  thread: ThreadId | null;
  /** Whether the open conversation is a draft (no thread yet), once the list is in. */
  drafting: boolean;
  /** The plan the rows are measured against: the oldest thread's (agent.ts's matterPlan). */
  plan: string | undefined;
  /** The agents the host lists (R15), for the plan names. */
  agents: readonly AgentSummary[];
  /** The composer's plan choice for a draft: the agents the host lists (R15), none with one or none. */
  options: readonly { value: AgentId; label: string }[];
  /** The agent the draft would run: the lawyer's choice, else agent.ts's order. */
  agent: AgentId;
  /** The lawyer's last move to a conversation (New conversation, a row pressed, a retry): its key, and a number that
   * grows with each, so the work puts the focus in the field once per move. Null after the room moved by itself. */
  focus: { key: string; n: number } | null;
  /** Whether Retry from here can be offered (H13). */
  canRetry: boolean;
  /** H13: the open thread was retried, in this room, from a question of another: that one's key and title, and the
   * question's time. Until H13 says it on the thread, a restart forgets it. */
  retried: { from: string; title?: string; at: Timestamp } | null;
};
