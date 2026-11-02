// The room's work column: the open thread and the composer under it. The thread reopens the matter's newest thread,
// or the first Ask makes one (session.ts). The thread's list, its rows and the Markdown renderer are one chunk of
// their own (chunk.ts, list.tsx), started as Work mounts; the session opens the thread only once it is in, so the
// rows never wait for it. The composer and the transcript's box are here, for the first paint.
import { Suspense, useEffect } from "react";
import { useStreamError, useStreamPart, useViewStore, useWatch } from "../../store/hooks.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { MatterSummary, ThreadId } from "../../transport/protocol.ts";
import { KeyedRequestContext, keyedRequest } from "../permission-keyed.ts";
import { LazyThreadList, loadThread } from "./chunk.ts";
import { useBusy } from "./live.ts";
import { ThreadActionsContext, useThreadSession, type Drafts } from "./session.ts";
import { WorkComposer } from "./work-composer.tsx";

/** The work column of the room: the matter's thread, and the composer (disabled with no matter). */
export function Work({
  transport,
  matter,
  drafts,
  onThread,
}: {
  transport: HostTransport;
  matter: MatterSummary | null;
  /** The room's composer drafts, by matter (room.tsx keeps them above Work's key). */
  drafts: Drafts;
  /** Hears the open thread: null until there is one. */
  onThread?: (thread: ThreadId | null) => void;
}) {
  const store = useViewStore();
  const session = useThreadSession(transport, store, matter?.matter ?? null);
  const stream = session.actions.stream;
  useWatch(store, stream);
  const busy = useBusy(stream, session.sentTurn);
  const failed = useStreamError(store, stream);
  // the one prompt that takes the keys, read here once a change rather than by every row
  const keyed = useStreamPart(store, stream, keyedRequest);
  useEffect(() => onThread?.(session.thread), [onThread, session.thread]);
  // the thread's chunk, in memory before the first view or the first answer, with a matter or without
  useEffect(() => void loadThread(), []);
  return (
    <>
      <ThreadActionsContext value={session.actions}>
        {/* the list's box, holding Thread, whose .room-thread is the scroller inside it; below 1000 px it takes the
            thread's place in the one column (responsive.css orders the thread first) */}
        <div data-testid="transcript" className="flex min-h-0 flex-1 flex-col max-mid:order-1">
          {/* empty until the thread's chunk is in: its arrival is one commit */}
          <Suspense fallback={null}>
            <KeyedRequestContext value={keyed}>
              <LazyThreadList transport={transport} stream={stream} busy={busy} />
            </KeyedRequestContext>
          </Suspense>
        </div>
      </ThreadActionsContext>
      <WorkComposer
        drafts={drafts}
        matter={matter?.matter ?? null}
        disabled={matter === null}
        busy={busy}
        refused={session.refused}
        failed={failed?.code ?? null}
        onAsk={session.ask}
        onStop={session.stop}
        onRetry={() => {
          if (stream !== null) store.retry(stream);
        }}
      />
    </>
  );
}
