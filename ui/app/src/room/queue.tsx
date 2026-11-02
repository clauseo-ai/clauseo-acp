// The wait over the composer while another conversation of the matter holds it (U7, matters/other): the parts'
// OtherConversationWait in the composer's queue slot, for the open conversation. After its message has gone (`queued
// { behind }`) it offers Stop the other alone; before, Queue too, which sends the field's words as Ask does, and the host
// makes the turn wait. No time shows: none is known (no end estimates). Stop the other cancels the other's turn; its
// answer so far stays in that conversation. A refused cancel's code goes to the composer's line (Work), with the
// next frame's flush, as the reply may land mid-stream (#253).
import { useRef } from "react";
import { OtherConversationWait } from "@clauseo/components/clauseo/queue-wait.tsx";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { useViewStore } from "../store/hooks.ts";
import { toHostError, type HostTransport } from "../transport/host-transport.ts";
import type { ErrorCode } from "../transport/protocol.ts";
import type { Conversations } from "./conversations.ts";
import { useOtherWait } from "./queue-model.ts";

const TEST_IDS = { section: "other-wait", stop: "stop-other", queue: "queue" } as const;

/** The wait for the conversation `conversationKey`, or nothing while no other conversation holds the matter. */
export function OtherWait({
  transport,
  conversations,
  conversationKey,
  busy,
  empty,
  onQueue,
  onRefused,
}: {
  transport: HostTransport;
  conversations: Conversations;
  /** The open conversation's key (conversations.ts). */
  conversationKey: string | null;
  /** The open conversation's own turn runs or is on its way: nothing to queue. */
  busy: boolean;
  /** The field holds no words: Queue shows, and takes no press. */
  empty: boolean;
  /** Queue: the field's words go, as Ask sends them. */
  onQueue: () => void;
  /** A refused Stop the other's code; null as another press goes. */
  onRefused: (code: ErrorCode | null) => void;
}) {
  const { lang } = useWords();
  const store = useViewStore();
  const wait = useOtherWait(conversations, conversationKey);
  // one cancel at a time: a second press while the first is on its way sends nothing
  const stopping = useRef(false);
  if (wait === null) return null;
  const stopOther = () => {
    if (stopping.current) return;
    stopping.current = true;
    onRefused(null);
    transport
      .dispatch({ command_id: crypto.randomUUID(), command: { cancel: { thread: wait.thread } } })
      .then(
        () => undefined,
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
      onQueue={wait.sent || busy ? undefined : onQueue}
      queueDisabled={empty}
      onStopOther={stopOther}
      testIds={TEST_IDS}
    />
  );
}
