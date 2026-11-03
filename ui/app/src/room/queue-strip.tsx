// The queue over the composer, filled from the thread's view (U7, matters/queue, queue-full, held, held-failed): the
// parts' QueueStrip with the queued messages in the order they go and why the queue holds. Its own chunk, which
// queue-slot.tsx loads, so the room's first load carries none of it. Edit sends the new text alone (the host keeps a
// message's file blocks), Remove, Send now and Send next their H9 commands, one at a time per row, and once accepted
// not again until the row has gone. Replies land with
// the next frame's flush (#253): a refusal shows its words under the head until the next press or until the queue
// changes; `thread.turn_not_queued` says the message had already gone, and the strip follows the view, where it now
// shows as sent. An edit refused so hands its words to the composer (`onEditLost`), never lost; when it was the last
// queued message (the strip gone or going with it) the composer also says it had gone. Never the host's message. An edit's key is its own
// (`edit:${turn}`), so Remove and Send now still go while it is on its way.
import { useEffect, useMemo, useRef, useState } from "react";
import { QueueStrip } from "@clauseo/components/clauseo/queue.tsx";
import type { QueueHold } from "@clauseo/components/clauseo/queue-model.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { useStreamPart, useViewStore } from "../store/hooks.ts";
import { dispatchH9, type HostCommandH9 } from "../transport/h9-pending.ts";
import { toHostError } from "../transport/host-transport.ts";
import type { TurnId } from "../transport/protocol.ts";
import type { Queued } from "./queue-entries.ts";
import { heldPick, queuedPick } from "./queue-picks.ts";
import type { QueueStripProps } from "./queue-slot.tsx";
import { useAgentPlan } from "./thread/agent-plan.ts";
import { stamp } from "./thread/clock.ts";

const TEST_IDS = {
  row: "queued",
  edit: "queued-edit",
  remove: "queued-remove",
  sendNow: "queued-send-now",
  sendNext: "send-next",
  say: "queue-say",
  note: "queue-error",
} as const;

const GONE = "thread.turn_not_queued";
/** Send next's key among the presses on their way (a row's is its turn). */
const NEXT = "next";

const NONE: readonly Queued[] = [];

/** A refused press: its code, and the queue as it stood then; the words show while the queue stands so. */
type Refusal = { code: string; queue: readonly Queued[] };

/** How a press's key is kept once the host accepts it: until its turn leaves the queue (Remove, Send now), until the
 * queue or its hold changes (Send next), or not at all (Edit, whose turn stays). */
type Keep = "turn" | "change" | "no";

export function LiveQueueStrip({
  transport,
  stream,
  thread,
  onFocusField,
  onEditLost,
  onSay,
}: QueueStripProps) {
  const store = useViewStore();
  const { lang } = useWords();
  const plan = useAgentPlan();
  const [pickQueued] = useState(queuedPick);
  const [pickHeld] = useState(heldPick);
  const queue = useStreamPart(store, stream, pickQueued);
  const held = useStreamPart(store, stream, pickHeld);
  const [refusal, setRefusal] = useState<Refusal | null>(null);
  // whether a refusal is held: a press makes no set to the null already there, which React would keep queued at the
  // click's lane and replay as a second commit in the reply's frame (#253, session.ts's useRefused)
  const refused = useRef(false);
  const refuse = (next: Refusal | null) => {
    if (next === null && !refused.current) return;
    refused.current = next !== null;
    setRefusal(next);
  };
  // one command per row at a time, by turn (Send next's by NEXT)
  const pending = useRef(new Map<string, Keep>());
  // an accepted press's key goes once its turn has left the queue, or the queue or its hold changed: a fast second
  // press before the flush that takes the row away sends nothing
  useEffect(() => {
    for (const [key, keep] of pending.current) {
      if (keep === "change" || (keep === "turn" && !queue.some((q) => q.turn === key)))
        pending.current.delete(key);
    }
  }, [queue, held]);
  // "now" as the strip first showed: no clock runs to move it
  const [now] = useState(Date.now);
  const hold = useMemo<QueueHold | null>(
    () =>
      held === null
        ? null
        : held.kind === "held"
          ? held
          : // the step's time, with its day when not today ("Monday 4:31 pm"), in the language
            { kind: held.kind, at: stamp(lang, held.at, now) },
    [held, lang, now],
  );
  const turnOf = (id: string): TurnId | undefined => queue.find((q) => q.id === id)?.turn;
  /** Sends one press's command, once per key while it is on its way or kept (`keep`). A refusal lands with the next
   * frame's flush: its words, and `refused` (the part's: the edit opens again, the pressed row is forgotten) unless
   * the message had already gone. */
  const send = (
    key: string,
    command: HostCommandH9,
    keep: Keep,
    refused?: () => void,
    gone?: (left: readonly Queued[]) => boolean,
  ) => {
    if (pending.current.has(key)) return;
    pending.current.set(key, keep);
    refuse(null);
    dispatchH9(transport, command).then(
      () => {
        if (keep === "no") pending.current.delete(key);
      },
      (e: unknown) => {
        pending.current.delete(key);
        const code = toHostError(e).code;
        store.inFrame(() => {
          // the queue as this frame's flush left it: the words stand until it changes
          const left = stream === null ? NONE : pickQueued(store.view(stream));
          // the message gone, and `gone` took the rest (an edit's words, to the composer): no words here
          if (code === GONE && gone?.(left) === true) return;
          refuse({ code, queue: left });
          if (code !== GONE) refused?.();
        });
      },
    );
  };
  const onRow =
    (command: (thread: string, turn: TurnId) => HostCommandH9) =>
    (id: string, refused: () => void): void => {
      const turn = turnOf(id);
      if (thread !== null && turn !== undefined) send(turn, command(thread, turn), "turn", refused);
    };
  const onEdit = (id: string, text: string, refused: () => void) => {
    const turn = turnOf(id);
    // the text alone: the host keeps the message's file blocks; its own key, so the row's Remove and Send now still go
    if (thread === null || turn === undefined) return;
    send(`edit:${turn}`, { edit_queued_prompt: { thread, turn, text } }, "no", refused, (left) => {
      // the message went with the edit on its way: its words are never lost, they go to the composer. With other
      // messages left the strip stays and says it had gone; with none, the strip is gone or going and the composer says it
      if (onEditLost === undefined) return false;
      const strip = left.some((q) => q.turn !== turn);
      onEditLost(text, !strip);
      return !strip;
    });
  };
  const onSendNext = () => {
    if (thread !== null) send(NEXT, { resume_queue: { thread } }, "change");
  };
  const note =
    refusal === null || refusal.queue !== queue
      ? null
      : {
          words:
            refusal.code === GONE
              ? mt(lang, "That message had already gone.")
              : errorWords(lang, refusal.code, plan),
          code: refusal.code,
        };
  return (
    <QueueStrip
      items={queue}
      held={hold}
      note={note}
      onEdit={onEdit}
      onRemove={onRow((t, turn) => ({ remove_queued_prompt: { thread: t, turn } }))}
      onSendNow={onRow((t, turn) => ({ send_queued_now: { thread: t, turn } }))}
      onSendNext={onSendNext}
      onFocusOut={onFocusField}
      onSay={onSay}
      testIds={TEST_IDS}
    />
  );
}
