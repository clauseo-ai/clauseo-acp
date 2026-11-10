// The conversations' view (conversations.ts), built from the open matter's state: its rows, newest first under the
// draft, the open thread, the plan and the agent a new thread runs, and what Retry offers. Pure: the class holds the
// state and calls these on every change. Apart from conversations.ts to keep that file under the size limit.
import type { AgentId, ThreadId } from "../transport/protocol.ts";
import { agentFor, matterPlan, planOptions } from "./agent.ts";
import type { MatterState } from "./conversations-state.ts";
import {
  newestFirst,
  noMatter,
  type ConversationRow,
  type ConversationsView,
} from "./conversations-view.ts";
import type { AgentSummary } from "./host-next.ts";

/** The thread behind conversation `key` in `m`: the thread holding that key, or the thread of that id. */
export function threadOf(m: MatterState, key: string): ThreadId | null {
  for (const [thread, k] of m.keys) if (k === key && m.threads.has(thread)) return thread;
  return m.threads.has(key) ? key : null;
}

/** The agent a new thread for `key` runs: its draft's choice, else agent.ts's order. */
export function agentOf(
  m: MatterState,
  agents: readonly AgentSummary[],
  key: string | null = m.open,
): AgentId {
  const chosen = m.draft !== null && m.draft.key === key ? m.draft.chosen : null;
  const threads = [...m.threads.values()].filter((t) => t.status !== "archived");
  return agentFor({ chosen, setting: null, threads, agents });
}

/** What the view needs beyond the matter: the agents read, whether the host offers Retry, and the focus asked for. */
export type MakeInput = {
  agents: AgentSummary[];
  retry: boolean;
  focus: { key: string; n: number } | null;
};

/** The view of `m`, the open matter's state, or of no matter open. */
export function makeView(
  m: MatterState | null,
  { agents, retry, focus }: MakeInput,
): ConversationsView {
  if (m === null) return noMatter(agents, retry);
  // an archived thread kept open (Find's) takes no row, whatever its header says; the first read's does (D-021)
  const hidden = m.kept?.status === "archived" ? m.kept.thread : null;
  const listed = [...m.threads.values()]
    .filter((t) => t.status !== "archived" && t.thread !== hidden)
    .sort(newestFirst);
  const options = planOptions(agents, listed);
  const rows: ConversationRow[] = listed.map((t) => ({
    key: m.keys.get(t.thread) ?? t.thread,
    thread: t.thread,
    ...(t.title === undefined ? {} : { title: t.title }),
    agent: t.agent,
    status: t.status,
    ...(t.seen === undefined ? {} : { seen: t.seen }),
    ...(t.last_stop === undefined ? {} : { last_stop: t.last_stop }),
    ...(t.snoozed_until === undefined ? {} : { snoozed_until: t.snoozed_until }),
    ...(t.done === true ? { done: true } : {}),
  }));
  const d = m.draft;
  if (d !== null && d.shown)
    rows.unshift({ key: d.key, thread: null, agent: null, status: "idle" });
  const thread = m.open === null ? null : threadOf(m, m.open);
  const origin = thread === null ? undefined : m.retried.get(thread);
  const source = origin === undefined ? undefined : m.threads.get(origin.from);
  // the host refuses Retry from here in an archived conversation, as it refuses a question
  const archived = thread !== null && m.threads.get(thread)?.status === "archived";
  return {
    matter: m.matter,
    phase: m.phase,
    refused: m.refused,
    rows,
    open: m.open,
    thread,
    drafting: m.phase !== "loading" && thread === null,
    archived,
    plan: matterPlan(listed, agents),
    agents,
    options,
    agent: agentOf(m, agents),
    focus: focus !== null && focus.key === m.open ? focus : null,
    canRetry: retry && !archived,
    retried:
      origin === undefined
        ? null
        : {
            from: m.keys.get(origin.from) ?? origin.from,
            ...(source?.title === undefined ? {} : { title: source.title }),
            at: origin.at,
          },
  };
}
