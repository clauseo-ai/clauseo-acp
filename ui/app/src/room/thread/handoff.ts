// The handover of a conversation to the other agent (host #1349): the reducer's "handoff" view entry, made from
// ContextHandoff and settled by HandoffSettled, goes with the lawyer's message it was built for (`message`), and the
// thread draws it just before that question (handoff-divider.tsx). This file finds a question's handoff in the view,
// names its two ends, and says whether Try again can redo it. Pure apart from the hook.
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
import { turnRunning } from "./live.ts";

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

/** Whether Try again redoes a failed handoff. The host has no command to retry one: a switch it committed whose turn
 * failed before the prompt went out owes the handover to the thread's next turn (#1349, switch.rs, `Kept::owed`), so
 * sending the question again carries it. That holds only while the thread is on the handoff's new agent (a handover
 * too long to fit committed no switch), nothing runs, and its question is still the newest. */
export function canTryAgain(v: StreamView | undefined, h: HandoffEntry): boolean {
  if (v === undefined || h.status !== "failed") return false;
  const head = v.header?.header;
  if (head === undefined || head.agent !== h.to.agent || turnRunning(head.status)) return false;
  let newest: EntryId | undefined;
  for (const id of v.ids) if (id.startsWith(USER)) newest = id;
  return newest === `${USER}${h.message}`;
}

/** The view's handoff for `message` and whether Try again redoes it, as one stable pick. */
export function useHandoff(
  stream: StreamId,
  message: MessageId,
): { entry: HandoffEntry; again: boolean } | null {
  const store = useViewStore();
  const entry = useStreamPart(store, stream, (v) => handoffFor(v, message));
  const again = useStreamPart(
    store,
    stream,
    useCallback(
      (v: StreamView | undefined) => {
        const h = handoffFor(v, message);
        return h !== undefined && canTryAgain(v, h);
      },
      [message],
    ),
  );
  return entry === undefined ? null : { entry, again };
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
