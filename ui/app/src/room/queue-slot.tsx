// The queue over the composer (U7, matters/queue, queue-full, held, held-failed): the composer's queue slot mounts this
// under the wait for another conversation. It reads two cheap parts of the view (how many wait, and whether the queue
// is held) and shows nothing while the queue is empty and not held; otherwise the strip, from its own chunk
// (queue-strip.tsx, with the parts' QueueStrip), so the room's first load carries neither. The chunk starts loading as
// the slot mounts (queue-chunk.ts), as the thread's does as Work mounts, so it is in memory before the first message
// can queue and the strip shows in the same frame as its first entry; a loaded chunk never suspends. Before it
// arrives the slot shows nothing. The slot keeps the strip's status region, empty and visually hidden, mounted for its
// whole life: a region that mounts holding words says nothing, so the strip says its head there (QueueStrip's onSay).
import { Suspense, useCallback, useEffect, useRef } from "react";
import { useStreamPart, useViewStore } from "../store/hooks.ts";
import type { StreamView } from "../store/view.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { StreamId, ThreadId } from "../transport/protocol.ts";
import { LazyQueueStrip, loadQueueStrip } from "./queue-chunk.ts";
import { queueCountPick } from "./queue-entries.ts";

export type QueueSlotProps = {
  transport: HostTransport;
  /** The open thread's stream and thread; null before the conversation has one. */
  stream: StreamId | null;
  thread: ThreadId | null;
  /** Puts the focus back in the composer's field (after a row's Send now or Remove takes the row away). */
  onFocusField: () => void;
  /** An edit refused as its message had already gone, with no other message left to show the strip: its words, for
   * the composer to keep and say so. */
  onEditLost?: ((text: string) => void) | undefined;
};

/** What the slot gives the strip: the contract's props, and where to say the strip's head. */
export type QueueStripProps = QueueSlotProps & { onSay: (said: string) => void };

const pickHeld = (v: StreamView | undefined) => v?.header?.header?.queue_held === true;

export function QueueSlot(props: QueueSlotProps) {
  const store = useViewStore();
  const count = useStreamPart(store, props.stream, queueCountPick);
  const held = useStreamPart(store, props.stream, pickHeld);
  useEffect(() => void loadQueueStrip(), []);
  // the strip's status region, mounted empty for the slot's life so the strip's first line is said as it comes
  const region = useRef<HTMLParagraphElement>(null);
  const say = useCallback((said: string) => {
    if (region.current !== null) region.current.textContent = said;
  }, []);
  return (
    <>
      <p ref={region} className="sr-only" role="status" data-testid="queue-said" />
      {(count > 0 || held) && (
        <Suspense fallback={null}>
          <LazyQueueStrip {...props} onSay={say} />
        </Suspense>
      )}
    </>
  );
}
