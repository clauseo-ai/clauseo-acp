// The queue over the composer, filled from the thread's view (U7, matters/queue, queue-full, held, held-failed): the
// parts' QueueStrip with the queued messages in the order they go and why the queue holds. Its own chunk, which
// queue-slot.tsx loads, so the room's first load carries none of it. Edit sends the new text alone (the host keeps a
// message's file blocks), Remove, Send now and Send next their H9 commands, one at a time per row. Replies land with
// the next frame's flush (#253): a refusal shows its words under the head until the next press or until the queue
// changes; `thread.turn_not_queued` says the message had already gone, and the strip follows the view, where it now
// shows as sent. Never the host's message.
import { useMemo, useRef, useState } from "react";
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
import type { QueueSlotProps } from "./queue-slot.tsx";
import { useAgentPlan } from "./thread/agent-plan.ts";

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

/** A step's time on the lawyer's clock, in English ("4:31 pm"), which the part puts in the language: thread/clock.ts's
 * clockTime in English, written out here so the strip's chunk shares no module with the thread's list (a shared one
 * becomes a chunk of its own, and the strip's import would need a preload; vite-config.test.ts). */
function clockOf(ts: number): string {
  const at = new Date(ts);
  const h = at.getHours();
  return `${h % 12 === 0 ? 12 : h % 12}:${String(at.getMinutes()).padStart(2, "0")} ${h < 12 ? "am" : "pm"}`;
}

/** A refused press: its code, and the queue as it stood then; the words show while the queue stands so. */
type Refusal = { code: string; queue: readonly Queued[] };

export function LiveQueueStrip({ transport, stream, thread, onFocusField }: QueueSlotProps) {
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
  const pending = useRef(new Set<string>());
  const hold = useMemo<QueueHold | null>(
    () =>
      held === null
        ? null
        : held.kind === "held"
          ? held
          : // the step's time on the lawyer's clock, in English: the part puts it in the language
            { kind: held.kind, at: clockOf(held.at) },
    [held],
  );
  const turnOf = (id: string): TurnId | undefined => queue.find((q) => q.id === id)?.turn;
  const send = (key: string, command: HostCommandH9) => {
    if (pending.current.has(key)) return;
    pending.current.add(key);
    refuse(null);
    dispatchH9(transport, command)
      .then(
        () => {},
        (e: unknown) => {
          const code = toHostError(e).code;
          store.inFrame(() =>
            // the queue as this frame's flush left it: the words stand until it changes
            refuse({ code, queue: stream === null ? NONE : pickQueued(store.view(stream)) }),
          );
        },
      )
      .finally(() => pending.current.delete(key));
  };
  const onRow =
    (command: (thread: string, turn: TurnId) => HostCommandH9) =>
    (id: string): void => {
      const turn = turnOf(id);
      if (thread !== null && turn !== undefined) send(turn, command(thread, turn));
    };
  const onEdit = (id: string, text: string) => {
    const turn = turnOf(id);
    // the text alone: the host keeps the message's file blocks
    if (thread !== null && turn !== undefined)
      send(turn, { edit_queued_prompt: { thread, turn, text } });
  };
  const onSendNext = () => {
    if (thread !== null) send(NEXT, { resume_queue: { thread } });
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
      testIds={TEST_IDS}
    />
  );
}
