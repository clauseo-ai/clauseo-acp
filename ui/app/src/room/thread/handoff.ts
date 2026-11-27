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
const STEP = "step:";

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

/** Where the thread stands for a handoff: on its new agent, its question still the newest (no later message has
 * carried it yet), and whether its question's turn failed with prompt.handoff_too_long (the host's own word that the
 * switch never happened: the conversation stayed with its agent). */
export type Standing = {
  readonly onNewAgent: boolean;
  readonly newest: boolean;
  readonly tooLong: boolean;
};

const TOO_LONG = "prompt.handoff_too_long";

/** Whether the turn of the handoff's question ended with prompt.handoff_too_long. */
export function tooLongOf(v: StreamView | undefined, h: HandoffEntry): boolean {
  if (v === undefined || h.status !== "failed") return false;
  const turn = v.entries.get(`${USER}${h.message}`)?.user_message?.turn;
  if (turn === undefined) return false;
  for (const id of v.ids) {
    if (!id.startsWith(STEP)) continue;
    const step = v.entries.get(id)?.step;
    if (step?.turn === turn && step.failure === TOO_LONG) return true;
  }
  return false;
}

export function standingOf(v: StreamView | undefined, h: HandoffEntry): Standing {
  const onNewAgent = v?.header?.header?.agent === h.to.agent;
  let newest: EntryId | undefined;
  if (v !== undefined) for (const id of v.ids) if (id.startsWith(USER)) newest = id;
  return { onNewAgent, newest: newest === `${USER}${h.message}`, tooLong: tooLongOf(v, h) };
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

/** The sentence under the divider: a key of words-thread.ts and its slot. */
export type Sentence = { readonly key: StatusKey; readonly slots: { agent: string } };
export type StatusKey =
  | "Clauseo couldn't confirm {agent} got the conversation so far, and sends it again by itself if {agent} starts afresh, so there's nothing to do."
  | "{agent} didn't get the conversation so far, so it goes with your next message."
  | "{agent} didn't get the conversation so far with this question; your next message goes as normal."
  | "Clauseo can't tell whether {agent} got the conversation so far; your next message goes as normal.";

/** The one map from a shown status to its one plain sentence and look, each saying what is true and what to do.
 * Unconfirmed: the host sends it again by itself to a new session of the agent (never to the one that most likely has
 * it), so nothing to do. Failed: owed to the next message only while the thread is on the new agent and no later
 * message has gone; else the next message goes as normal. A handover too long (standing.tooLong) is no switch: the
 * divider says the conversation stayed (handoff-divider.tsx), with no sentence here. */
export function statusLine(
  shown: Shown,
  agent: string,
  standing: Standing,
): { readonly tone: "quiet" | "amber" | "red"; readonly sentence: Sentence | null } {
  const slots = { agent };
  if (standing.tooLong) return { tone: "quiet", sentence: null };
  switch (shown) {
    case "pending":
    case "delivered":
      return { tone: "quiet", sentence: null };
    case "unconfirmed":
      return {
        tone: "amber",
        sentence: {
          key: "Clauseo couldn't confirm {agent} got the conversation so far, and sends it again by itself if {agent} starts afresh, so there's nothing to do.",
          slots,
        },
      };
    case "failed":
      return {
        tone: "red",
        sentence:
          standing.onNewAgent && standing.newest
            ? {
                key: "{agent} didn't get the conversation so far, so it goes with your next message.",
                slots,
              }
            : {
                key: "{agent} didn't get the conversation so far with this question; your next message goes as normal.",
                slots,
              },
      };
    case "unknown":
      return {
        tone: "amber",
        sentence: {
          key: "Clauseo can't tell whether {agent} got the conversation so far; your next message goes as normal.",
          slots,
        },
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
  const tooLong = useStreamPart(store, stream, (v) => {
    const h = handoffFor(v, message);
    return h !== undefined && tooLongOf(v, h);
  });
  return entry === undefined ? null : { entry, standing: { onNewAgent, newest, tooLong } };
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
