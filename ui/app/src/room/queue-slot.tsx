// The queue over the composer (U7, matters/queue, queue-full, held, held-failed): the composer's queue slot mounts this
// under the wait for another conversation. A stub until the strip lands; its props are the contract.
import type { HostTransport } from "../transport/host-transport.ts";
import type { StreamId, ThreadId } from "../transport/protocol.ts";

export type QueueSlotProps = {
  transport: HostTransport;
  /** The open thread's stream and thread; null before the conversation has one. */
  stream: StreamId | null;
  thread: ThreadId | null;
  /** Puts the focus back in the composer's field (after a row's Send now or Remove takes the row away). */
  onFocusField: () => void;
};

export function QueueSlot(_props: QueueSlotProps) {
  return null;
}
