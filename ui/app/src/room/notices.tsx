// What the room says about Clauseo itself, in the one place room.tsx puts it (the rail's foot, or the wall's head where
// the rail folds away): the notice after Clauseo closed unexpectedly (crash-notice.tsx), then R23's update notice
// (update-notice.tsx). Each shows nothing when it has nothing to say.
import type { HostTransport } from "../transport/host-transport.ts";
import type { MatterSummary } from "../transport/protocol.ts";
import type { AllThreads } from "./all-threads.ts";
import { CrashNotice } from "./crash-notice.tsx";
import { UpdateNotice } from "./update-notice.tsx";

export function RoomNotices({
  transport,
  threads,
  matters,
}: {
  transport: HostTransport;
  /** Every matter's threads, for whether a turn runs anywhere. */
  threads: AllThreads | null;
  /** The room's matters, for the names the crash notice gives. */
  matters: readonly MatterSummary[];
}) {
  return (
    <>
      <CrashNotice transport={transport} matters={matters} />
      <UpdateNotice transport={transport} threads={threads} />
    </>
  );
}
