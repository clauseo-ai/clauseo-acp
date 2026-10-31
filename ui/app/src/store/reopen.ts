// What the window opens on start: the matter with the latest activity and its last thread. `MatterSummary` and
// `ThreadSummary` carry no activity time yet, so until a `last_activity` field arrives, "latest" is the newest
// created: the greatest id, since ids are v7 UUIDs, whose text sorts by creation time. This is the one place that
// changes when the field arrives.
import type { MatterSummary, ThreadSummary } from "../transport/protocol.ts";

function newest<T>(items: readonly T[], id: (item: T) => string): T | null {
  let best: T | null = null;
  for (const item of items) if (best === null || id(item) > id(best)) best = item;
  return best;
}

/** The matter to reopen, or null when there is none. */
export const pickReopenMatter = (matters: readonly MatterSummary[]): MatterSummary | null =>
  newest(matters, (m) => m.matter);

/** The thread to reopen in a matter: the newest that is not archived, or null. */
export const pickReopenThread = (threads: readonly ThreadSummary[]): ThreadSummary | null =>
  newest(
    threads.filter((t) => t.status !== "archived"),
    (t) => t.thread,
  );
