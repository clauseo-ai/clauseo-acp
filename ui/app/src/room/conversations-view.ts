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
import { agentFor, planOptions } from "./agent.ts";
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
  /** H-triage: when its snooze ends, and whether it's done (U16b); absent: neither. */
  snoozed_until?: Timestamp;
  done?: boolean;
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
  /** Whether the open conversation is archived (Find opens one): read only, the host refusing a question to it. */
  archived: boolean;
  /** The plan the rows are measured against: the one the matter uses most (agent.ts's matterPlan). */
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

/** Whether a thread is at rest: not starting, answering or waiting on the lawyer. */
export const resting = (s: ThreadStatus) =>
  !(s === "starting" || s === "running" || s === "needs_you");

/** Newest activity first: H6's last activity, else the newest created (v7 ids sort by creation). */
export const newestFirst = (a: Known, b: Known) => {
  const at = (b.last_activity ?? -Infinity) - (a.last_activity ?? -Infinity);
  if (at !== 0 && !Number.isNaN(at)) return at;
  return a.thread < b.thread ? 1 : a.thread > b.thread ? -1 : 0;
};

/** The view with no matter open. */
export const noMatter = (
  agents: readonly AgentSummary[],
  canRetry: boolean,
): ConversationsView => ({
  matter: null,
  phase: "loading",
  refused: null,
  rows: [],
  open: null,
  thread: null,
  drafting: false,
  archived: false,
  plan: undefined,
  agents,
  options: planOptions(agents),
  agent: agentFor({ chosen: null, setting: null, threads: [], agents }),
  focus: null,
  canRetry,
  retried: null,
});
