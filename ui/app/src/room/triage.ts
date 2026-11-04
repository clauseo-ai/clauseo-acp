// The rail's triage, pure (no React): from the matters, every matter's threads (all-threads.ts) and the open matter to
// the groups and what each matter's row says, with the thread behind it. As the approved page's table has it:
// - a matter needs you while any thread waits on the lawyer's hand (needs_you), has an answer ready and unread (idle,
//   not seen, its last turn not failed, stopped at the limit or cancelled), or failed and isn't seen yet;
// - its row shows the most pressing: a wait on the hand, then an answer ready, then a failure. ThreadStatus doesn't say
//   permission or question yet, so the words come from the matter's status (H5) and the row names the oldest waiting
//   thread; ready and failed name the oldest such thread;
// - working, queued, a limit pause (until U14 gives its open choice), a stop, and nothing at all stay in the date groups
//   with the matter's own status, muted;
// - Pinned keeps a pinned matter, its line and all; Needs you sits under Pinned, above the date groups, ordered by
//   kind (the hand's waits, then answers ready, then failures), each kind in the order its rows came: a row that comes
//   later joins at the kind's foot.
// Where a matter's thread list hasn't come (no app feed), the matter's status alone places it: a question, a permission
// or an answer ready need you, with no thread named. `held` keeps rows where they stand: the open matter until another
// opens, and every row while the pointer is over the rail (rail-triage.ts keeps both); a row's words still change.
// U16b: a conversation snoozed (its snoozed_until still ahead of now) or done counts for nothing here: a wake (the
// reducer clearing it) brings its need back. A matter whose every conversation not done is snoozed to one time is
// snoozed whole: it leaves the groups for the Snoozed shelf (`shelf`), unless pinned or held. Matters are never done.
import type { MatterId, ThreadStatus, ThreadSummary, Timestamp } from "../transport/protocol.ts";
import type { MatterInfo } from "./host-next.ts";
import type { RowState, RowStatus } from "./matters.ts";

/** Why a matter needs the lawyer, in Needs you's order. */
export type Need = "hand" | "ready" | "failed";
/** Where a matter's row stands. "date": its date group, by its next date. */
export type Group = "pinned" | "needs" | "date" | "snoozed";

/** A row's state: the matters' row states and Failed. */
export type TriageState = RowState | "failed";
export type TriageStatus = { state: TriageState; at?: Timestamp };

export type TriageRow = {
  /** What the row says; undefined: nothing. */
  status: TriageStatus | undefined;
  need: Need | null;
  /** The conversation behind a row that needs you, where its list is known. */
  thread: ThreadSummary | null;
};

export type Triage = {
  rows: ReadonlyMap<MatterId, TriageRow>;
  group: ReadonlyMap<MatterId, Group>;
  /** Needs you's rows, in order. */
  needs: readonly MatterId[];
  /** When each row in Needs you came, for the next triage, and its kind's rank last known. */
  arrived: ReadonlyMap<MatterId, Arrival>;
};

/** When a row came into Needs you (a number that grows with each arrival), and the rank of its kind last known: a row
 * held there with nothing left to need keeps its place. */
export type Arrival = { n: number; rank: number };

export type TriageInput = {
  /** The folder matters (the scratch matter stands at the rail's foot, apart). */
  matters: readonly MatterInfo[];
  /** Each matter's own status (matters.ts: H5's, or its held threads'). */
  status: ReadonlyMap<MatterId, RowStatus>;
  lists: ReadonlyMap<MatterId, readonly ThreadSummary[]>;
  /** Rows that keep their group, whatever they now say. */
  held: ReadonlyMap<MatterId, Group> | null;
  /** The previous triage's arrivals. */
  arrived: ReadonlyMap<MatterId, Arrival>;
  /** The time snoozes are compared with: a snooze whose time has come counts as none. */
  now: Timestamp;
};

/** Snoozed: its time to come back still ahead. The host clears it at that time; comparing covers a render before. */
export const snoozed = (t: Pick<ThreadSummary, "snoozed_until">, now: Timestamp): boolean =>
  t.snoozed_until !== undefined && t.snoozed_until > now;

/** The time a matter is snoozed to whole (every thread of it neither done nor archived snoozed to that one time),
 * else null; a matter with no such thread isn't snoozed. */
export function wholly(
  list: readonly ThreadSummary[] | undefined,
  now: Timestamp,
): Timestamp | null {
  let until: Timestamp | null = null;
  for (const t of list ?? []) {
    if (t.status === "archived" || t.done === true) continue;
    if (!snoozed(t, now) || (until !== null && t.snoozed_until !== until)) return null;
    until = t.snoozed_until ?? null;
  }
  return until;
}

/** Whether a matter other than `open` waits on the lawyer's hand, a question or a permission, by its status (H5),
 * where no conversation of its list waits, or where every one that does is snoozed or done. */
export function handWaits(
  status: ReadonlyMap<MatterId, RowStatus>,
  open: MatterId | null,
  lists: ReadonlyMap<MatterId, readonly ThreadSummary[]>,
  now: Timestamp,
): boolean {
  for (const [id, s] of status) {
    if (id === open || (s.state !== "question" && s.state !== "permission")) continue;
    const list = lists.get(id);
    if (
      list === undefined ||
      list.some((t) => t.status === "needs_you" && t.done !== true && !snoozed(t, now))
    )
      return true;
  }
  return false;
}

const RANK: Record<Need, number> = { hand: 0, ready: 1, failed: 2 };

/** A thread at rest: nothing runs in it or waits on it. */
const resting = (s: ThreadStatus) => s === "idle" || s === "agent_exited" || s === "idle_stopped";

/** Why a thread needs the lawyer, or null: the page's table. */
export function needOf(t: Pick<ThreadSummary, "status" | "seen" | "last_stop">): Need | null {
  if (t.status === "needs_you") return "hand";
  if (!resting(t.status) || t.seen !== false || t.last_stop === undefined) return null;
  const reason = t.last_stop.reason;
  if (reason === "failed") return "failed";
  if (t.status === "idle" && reason !== "at_limit" && reason !== "cancelled") return "ready";
  return null;
}

/** Oldest first: the earlier moment (the stop for ready and failed, the last activity for a wait), then the older id. */
const momentOf = (t: ThreadSummary, need: Need) =>
  (need === "hand" ? t.last_activity : t.last_stop?.at) ?? t.last_activity ?? Infinity;

function rowOf(
  m: MatterInfo,
  own: RowStatus | undefined,
  list: readonly ThreadSummary[] | undefined,
  now: Timestamp,
): TriageRow {
  if (list === undefined) {
    const s = own?.state;
    const need: Need | null =
      s === "question" || s === "permission" ? "hand" : s === "ready" ? "ready" : null;
    return { status: own, need, thread: null };
  }
  let best: { t: ThreadSummary; need: Need } | null = null;
  for (const t of list) {
    const need = t.done === true || snoozed(t, now) ? null : needOf(t);
    if (need === null) continue;
    if (
      best === null ||
      RANK[need] < RANK[best.need] ||
      (RANK[need] === RANK[best.need] &&
        (momentOf(t, need) < momentOf(best.t, best.need) ||
          (momentOf(t, need) === momentOf(best.t, best.need) && t.thread < best.t.thread)))
    )
      best = { t, need };
  }
  // a folder that moved says so over anything
  const moved = own?.state === "moved";
  if (best === null) {
    // the matter's status says what it's doing, but not a need the threads no longer show
    const s = own?.state;
    const quiet = s === "question" || s === "permission" || s === "ready";
    return { status: quiet ? undefined : own, need: null, thread: null };
  }
  const status: TriageStatus = moved
    ? { state: "moved" }
    : best.need === "hand"
      ? { state: m.status === "needs_permission" ? "permission" : "question" }
      : best.need === "ready"
        ? { state: "ready" }
        : {
            state: "failed",
            ...(best.t.last_stop === undefined ? {} : { at: best.t.last_stop.at }),
          };
  return { status, need: best.need, thread: best.t };
}

/** The groups and every row's words. */
export function triage({ matters, status, lists, held, arrived, now }: TriageInput): Triage {
  const rows = new Map<MatterId, TriageRow>();
  const group = new Map<MatterId, Group>();
  const needing: { m: MatterInfo; row: TriageRow }[] = [];
  for (const m of matters) {
    const list = lists.get(m.matter);
    const row = rowOf(m, status.get(m.matter), list, now);
    rows.set(m.matter, row);
    const g: Group = m.pinned
      ? "pinned"
      : (held?.get(m.matter) ??
        (m.kind !== "scratch" && wholly(list, now) !== null
          ? "snoozed"
          : row.need === null
            ? "date"
            : "needs"));
    group.set(m.matter, g);
    if (g === "needs") needing.push({ m, row });
  }
  const next = Math.max(-1, ...[...arrived.values()].map((a) => a.n)) + 1;
  const came = new Map<MatterId, Arrival>();
  for (const { m, row } of needing) {
    const was = arrived.get(m.matter);
    const rank = row.need === null ? (was?.rank ?? 3) : RANK[row.need];
    // a row whose kind changes comes again, at its new kind's foot
    came.set(m.matter, { n: was !== undefined && was.rank === rank ? was.n : next, rank });
  }
  const at = (n: { m: MatterInfo; row: TriageRow }) =>
    n.row.thread === null || n.row.need === null ? Infinity : momentOf(n.row.thread, n.row.need);
  const of = (n: { m: MatterInfo }) => came.get(n.m.matter) ?? { n: 0, rank: 3 };
  needing.sort(
    (a, b) =>
      of(a).rank - of(b).rank ||
      of(a).n - of(b).n ||
      at(a) - at(b) ||
      (a.m.matter < b.m.matter ? -1 : a.m.matter > b.m.matter ? 1 : 0),
  );
  return { rows, group, needs: needing.map((n) => n.m.matter), arrived: came };
}
