// The Snoozed shelf's rows, pure (U16b), loaded with the later chunk (snooze-late.tsx): triage.ts's rules for a
// snoozed conversation and a matter snoozed whole.
import type { MatterId, ThreadSummary, Timestamp } from "../transport/protocol.ts";
import type { MatterInfo } from "./host-next.ts";
import { snoozed, wholly, type Group } from "./triage.ts";

/** One row of the Snoozed shelf: a matter snoozed whole (thread null) or a conversation. */
export type Shelved = { matter: MatterInfo; thread: ThreadSummary | null; until: Timestamp };

/** The Snoozed shelf, the next back first: each matter snoozed whole as one row, else each snoozed conversation (the
 * scratch matter's one by one). A matter snoozed whole that stays in a group (`group`: Pinned, or held there) isn't on
 * the shelf at all: its row says it's snoozed. */
export function shelf(
  matters: readonly MatterInfo[],
  lists: ReadonlyMap<MatterId, readonly ThreadSummary[]>,
  now: Timestamp,
  group: ReadonlyMap<MatterId, Group> = new Map(),
): Shelved[] {
  const rows: Shelved[] = [];
  for (const matter of matters) {
    const list = lists.get(matter.matter) ?? [];
    const whole = matter.kind === "scratch" ? null : wholly(list, now);
    const placed = group.get(matter.matter);
    if (whole !== null && placed !== undefined && placed !== "snoozed") continue;
    if (whole !== null) rows.push({ matter, thread: null, until: whole });
    else
      for (const t of list)
        if (t.status !== "archived" && t.snoozed_until !== undefined && snoozed(t, now))
          rows.push({ matter, thread: t, until: t.snoozed_until });
  }
  return rows.sort((a, b) => a.until - b.until);
}
