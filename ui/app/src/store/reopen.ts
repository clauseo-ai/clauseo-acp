// What the window opens on start: the matter opened last, or with the latest activity, and its last thread. Today's
// `MatterSummary` and `ThreadSummary` carry no times, so without them "latest" is the newest created: the greatest id,
// since ids are v7 UUIDs, whose text sorts by creation time. With H5's `last_opened` and `last_activity` (a NextHost's
// MatterInfo, room/host-next.ts) those come first. This is the one place that orders them.
import type { MatterSummary, ThreadSummary, Timestamp } from "../transport/protocol.ts";

function newest<T>(items: readonly T[], id: (item: T) => string): T | null {
  let best: T | null = null;
  for (const item of items) if (best === null || id(item) > id(best)) best = item;
  return best;
}

/** A matter with H5's times, where the host gives them. */
type Timed = MatterSummary & { last_opened?: Timestamp; last_activity?: Timestamp };

/** Whether `a` reopens before `b`: opened later, then active later, then newer. */
const before = (a: Timed, b: Timed): boolean => {
  const opened = [a.last_opened ?? -Infinity, b.last_opened ?? -Infinity] as const;
  if (opened[0] !== opened[1]) return opened[0] > opened[1];
  const active = [a.last_activity ?? -Infinity, b.last_activity ?? -Infinity] as const;
  if (active[0] !== active[1]) return active[0] > active[1];
  return a.matter > b.matter;
};

/** The matter to reopen, or null when there is none. */
export function pickReopenMatter<M extends Timed>(matters: readonly M[]): M | null {
  let best: M | null = null;
  for (const m of matters) if (best === null || before(m, best)) best = m;
  return best;
}

/** The thread to reopen in a matter: the newest that is not archived, or null. */
export const pickReopenThread = (threads: readonly ThreadSummary[]): ThreadSummary | null =>
  newest(
    threads.filter((t) => t.status !== "archived"),
    (t) => t.thread,
  );
