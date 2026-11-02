// The wait over the composer while another conversation of the matter holds it (U7, matters/other, design
// state.ts's ask/queueBehind/stopOther): the parts' OtherConversationWait in the composer's queue slot. Ask pressed
// with words while another conversation holds the matter raises it and sends nothing (work-composer.tsx); Queue then
// sends the field's words as Ask would, and the host makes the turn wait (`queued { behind }`), when the panel offers
// Stop the other alone and the composer's working line says the wait. No time shows: none is known (no end
// estimates). Stop the other cancels the other's turn, whose answer so far stays in that conversation; pressed before
// the message went, the message then goes, as the design has it. A refused cancel's code goes to the composer's line
// (Work), and every reply lands with the next frame's flush, as it may come mid-stream (#253).
import { useRef } from "react";
import { OtherConversationWait } from "@clauseo/components/clauseo/queue-wait.tsx";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { useViewStore } from "../store/hooks.ts";
import { toHostError, type HostTransport } from "../transport/host-transport.ts";
import type { ErrorCode } from "../transport/protocol.ts";
import type { OtherWait as Wait } from "./queue-model.ts";

const TEST_IDS = {
  section: "other-wait",
  stop: "stop-other",
  queue: "queue",
  said: "other-wait-said",
} as const;

/** The wait for `wait`'s conversation: Stop the other, and Queue until the message has gone. */
export function OtherWait({
  transport,
  wait,
  busy,
  empty,
  onQueue,
  onFocusField,
  onRefused,
}: {
  transport: HostTransport;
  wait: Wait;
  /** The open conversation's own message is on its way: nothing to queue. */
  busy: boolean;
  /** The field holds no words: Queue shows, and takes no press. */
  empty: boolean;
  /** Queue: the field's words go, as Ask sends them. */
  onQueue: () => void;
  /** Puts the focus in the composer's field. */
  onFocusField: () => void;
  /** A refused Stop the other's code; null as another press goes. */
  onRefused: (code: ErrorCode | null) => void;
}) {
  const { lang } = useWords();
  const store = useViewStore();
  const stop = useRef<HTMLButtonElement>(null);
  // one cancel at a time: a second press while the first is on its way sends nothing
  const stopping = useRef(false);
  const queueable = !wait.sent && !busy;
  const queue = () => {
    onQueue();
    // Queue goes once the message is on its way: the focus goes to Stop the other, which stays, never to the page
    if (stop.current !== null) stop.current.focus();
    else onFocusField();
  };
  const stopOther = () => {
    if (stopping.current) return;
    stopping.current = true;
    onRefused(null);
    // pressed before the message went: once the other stops, the message goes (design state.ts, stopOther)
    const thenSend = queueable && !empty;
    transport
      .dispatch({ command_id: crypto.randomUUID(), command: { cancel: { thread: wait.thread } } })
      .then(
        () => {
          if (thenSend) store.inFrame(onQueue);
        },
        (e: unknown) => {
          const code = toHostError(e).code;
          store.inFrame(() => onRefused(code));
        },
      )
      .finally(() => {
        stopping.current = false;
      });
  };
  return (
    <OtherConversationWait
      title={wait.title ?? mt(lang, "New conversation")}
      onQueue={queueable ? queue : undefined}
      queueDisabled={empty}
      onStopOther={stopOther}
      stopRef={stop}
      testIds={TEST_IDS}
    />
  );
}
