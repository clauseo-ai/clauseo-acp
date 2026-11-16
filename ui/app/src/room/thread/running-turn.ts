// Which turn of the open thread runs, for the connectors store's running-low line (low-credit.tsx): the thread
// list calls useRunningTurn once, so the store hears each turn's start and end while the thread is open.
import { useEffect } from "react";
import { connectorsStoreOf } from "../../store/connectors.ts";
import { useStreamPart, useViewStore } from "../../store/hooks.ts";
import type { StreamView } from "../../store/view.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { StreamId } from "../../transport/protocol.ts";
import type { Exchange } from "./exchanges.ts";
import { turnRunning } from "./live.ts";

const THREAD = "thread:";
const STEP = "step:";

const pickRunning = (v: StreamView | undefined) => turnRunning(v?.header?.header?.status);

/** Tells the connectors store which turn of the open thread runs: the latest exchange's, while the thread's status
 * says a turn runs and that exchange has no step yet. Seen running on open counts as started then. */
export function useRunningTurn(
  transport: HostTransport,
  stream: StreamId | null,
  latest: Exchange | undefined,
) {
  const running = useStreamPart(useViewStore(), stream, pickRunning);
  const turn =
    running &&
    latest !== undefined &&
    latest.turn !== null &&
    !latest.ids.some((id) => id.startsWith(STEP))
      ? latest.turn
      : null;
  const thread = stream !== null && stream.startsWith(THREAD) ? stream.slice(THREAD.length) : null;
  useEffect(() => {
    if (thread === null || turn === null) return;
    const store = connectorsStoreOf(transport);
    store.turnStarted(thread, turn);
    return () => store.turnEnded(thread, turn);
  }, [transport, thread, turn]);
}
