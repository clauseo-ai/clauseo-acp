// The handover of a conversation to the other agent (host #1349): the reducer's "handoff" view entry, made from
// ContextHandoff and settled by HandoffSettled, goes with the lawyer's message it was built for (`message`), and the
// thread draws it just before that question (handoff-divider.tsx). This file finds a question's handoff in the view,
// names its two ends, and says how a settled one went (statusLine). Pure apart from the hook.
import { useCallback } from "react";
import { useStreamPart, useViewStore } from "../../store/hooks.ts";
import type { StreamView } from "../../store/view.ts";
import type {
  Endpoint,
  EntryId,
  HandoffEntry,
  MessageId,
  StreamId,
} from "../../transport/protocol.ts";
import { kindOf } from "../agent.ts";

const HANDOFF = "handoff:";
const USER = "user_message:";

/** Each handoff's entry id by the message it goes with, one map per id list. */
const byMessageOf = new WeakMap<readonly EntryId[], ReadonlyMap<MessageId, EntryId>>();

function byMessage(v: StreamView): ReadonlyMap<MessageId, EntryId> {
  let map = byMessageOf.get(v.ids);
  if (map === undefined) {
    const built = new Map<MessageId, EntryId>();
    for (const id of v.ids) {
      if (!id.startsWith(HANDOFF)) continue;
      const h = v.entries.get(id)?.handoff;
      if (h !== undefined) built.set(h.message, id);
    }
    map = built;
    byMessageOf.set(v.ids, map);
  }
  return map;
}

/** The handoff that goes with `message`, or undefined. */
export function handoffFor(
  v: StreamView | undefined,
  message: MessageId,
): HandoffEntry | undefined {
  if (v === undefined) return undefined;
  const id = byMessage(v).get(message);
  return id === undefined ? undefined : v.entries.get(id)?.handoff;
}

/** Where the thread stands for a handoff: on its new agent (a switch committed: a handover too long to fit commits
 * none), and its question still the newest (no later message has carried it yet). */
export type Standing = { readonly onNewAgent: boolean; readonly newest: boolean };

export function standingOf(v: StreamView | undefined, h: HandoffEntry): Standing {
  const onNewAgent = v?.header?.header?.agent === h.to.agent;
  let newest: EntryId | undefined;
  if (v !== undefined) for (const id of v.ids) if (id.startsWith(USER)) newest = id;
  return { onNewAgent, newest: newest === `${USER}${h.message}` };
}

/** A status as the divider says it, the host's own (HandoffEntryStatus): "unconfirmed" (the prompt went out, the
 * turn ended before the new agent answered), "failed" (it never went out, or the agent refused it), or "unknown" for
 * one from a newer Clauseo. */
export type Shown = "pending" | "delivered" | "unconfirmed" | "failed" | "unknown";

/** The status to show: the host's, straight through. */
export function shownOf(status: string): Shown {
  if (
    status === "pending" ||
    status === "delivered" ||
    status === "unconfirmed" ||
    status === "failed"
  )
    return status;
  return "unknown";
}

/** A switch back (ContextHandoff.resumed) that passes nothing on: the agent's own session had seen it all. */
export function passesNothing(h: HandoffEntry): boolean {
  return (
    h.resumed === true &&
    h.answers_covered === 0 &&
    h.items_included === 0 &&
    h.items_left_out === 0
  );
}

/** One sentence of a status line: a key of words-thread.ts and its slots. */
export type Sentence = { readonly key: StatusKey; readonly slots?: { agent: string } };
export type StatusKey =
  | "Clauseo couldn't confirm the handover reached {agent}."
  | "The handover didn't go out."
  | "It's sent with your next message.";

/** The one map from a shown status to its words and look. The "next message" sentence stands only while it is true:
 * the thread is on the new agent, which the host owes the handover, and no later message has carried it. An
 * unconfirmed one is not owed again (the agent most likely has it), so it promises nothing. */
export function statusLine(
  shown: Shown,
  agent: string,
  standing: Standing,
): { readonly tone: "quiet" | "amber" | "red"; readonly sentences: readonly Sentence[] } {
  const next = standing.onNewAgent && standing.newest;
  switch (shown) {
    case "pending":
    case "delivered":
      return { tone: "quiet", sentences: [] };
    case "unconfirmed":
      return {
        tone: "amber",
        sentences: [
          { key: "Clauseo couldn't confirm the handover reached {agent}.", slots: { agent } },
        ],
      };
    case "failed":
      return {
        tone: "red",
        sentences: [
          { key: "The handover didn't go out." },
          ...(next ? [{ key: "It's sent with your next message." } as const] : []),
        ],
      };
    case "unknown":
      return {
        tone: "amber",
        sentences: next ? [{ key: "It's sent with your next message." }] : [],
      };
  }
}

/** The view's handoff for `message` and where the thread stands for it. */
export function useHandoff(
  stream: StreamId,
  message: MessageId,
): { entry: HandoffEntry; standing: Standing } | null {
  const store = useViewStore();
  const entry = useStreamPart(store, stream, (v) => handoffFor(v, message));
  const onNewAgent = useStreamPart(store, stream, (v) => {
    const h = handoffFor(v, message);
    return h !== undefined && standingOf(v, h).onNewAgent;
  });
  const newest = useStreamPart(
    store,
    stream,
    useCallback(
      (v: StreamView | undefined) => {
        const h = handoffFor(v, message);
        return (
          h !== undefined &&
          h.status !== "pending" &&
          h.status !== "delivered" &&
          standingOf(v, h).newest
        );
      },
      [message],
    ),
  );
  return entry === undefined ? null : { entry, standing: { onNewAgent, newest } };
}

/** "Claude" or "ChatGPT" for an agent of a kind, else its own id. */
export function agentWord(agent: Endpoint["agent"]): string {
  const kind = kindOf(agent, []);
  if (kind === "claude") return "Claude";
  if (kind === "codex") return "ChatGPT";
  return agent;
}

/** One end as one run of words, only what is known: "Claude Opus 5.5", or the agent alone. */
export function endText(end: Endpoint): string {
  const model = end.model?.name.trim() ?? "";
  return model === "" ? agentWord(end.agent) : `${agentWord(end.agent)} ${model}`;
}
