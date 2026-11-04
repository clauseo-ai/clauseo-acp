// triage.ts with H-triage's marks (U16b), pure: a snoozed or done conversation counts for nothing in Needs you, a
// snooze whose time has come counts as none, a wake brings the row back with its status, a matter snoozed whole leaves
// the groups for the shelf, and the shelf's rows, the next back first.
import { expect, test } from "vitest";
import type { ThreadSummary } from "../transport/protocol.ts";
import { uuid } from "../testing/stream-script.ts";
import { detailed } from "./fake-next.ts";
import type { MatterInfo } from "./host-next.ts";
import { shelf, triage, wholly } from "./triage.ts";

const NOW = Date.UTC(2026, 10, 3, 17, 0);
const LATER = NOW + 3_600_000;
const matter = (n: number, over: Partial<MatterInfo> = {}) =>
  detailed({ matter: uuid(n, 2), name: `Matter ${n}`, kind: "folder" }, over);
const thread = (n: number, m: MatterInfo, over: Partial<ThreadSummary> = {}): ThreadSummary => ({
  thread: uuid(n, 3),
  matter: m.matter,
  agent: "claude-code",
  status: "idle",
  seen: true,
  last_activity: NOW - n * 1000,
  ...over,
});
const run = (ms: readonly MatterInfo[], lists: [MatterInfo, ThreadSummary[]][], now = NOW) =>
  triage({
    matters: ms,
    status: new Map(),
    lists: new Map(lists.map(([m, ts]) => [m.matter, ts])),
    held: null,
    arrived: new Map(),
    now,
  });

const A = matter(1, { status: "needs_answer" });
const B = matter(2);

test("a snoozed conversation doesn't count towards Needs you, and its matter says nothing", () => {
  const asking = thread(1, A, { status: "needs_you", snoozed_until: LATER });
  const t = run([A, B], [[A, [asking, thread(2, A)]]]);
  expect(t.needs).toEqual([]);
  expect(t.group.get(A.matter)).toBe("date");
  expect(t.rows.get(A.matter)?.status).toBeUndefined();
});

test("a snooze whose time has come counts as none: the row is back at the first triage after it, not before", () => {
  const asking = thread(1, A, { status: "needs_you", snoozed_until: LATER });
  const lists: [MatterInfo, ThreadSummary[]][] = [[A, [asking, thread(2, A)]]];
  expect(run([A], lists, LATER - 1).needs).toEqual([]);
  expect(run([A], lists, LATER).needs).toEqual([A.matter]);
  expect(run([A], lists, LATER).rows.get(A.matter)?.status).toEqual({ state: "question" });
});

test("the wake: a snooze the reducer cleared brings the row back to Needs you with its status", () => {
  const asking = thread(1, A, { status: "needs_you", snoozed_until: LATER });
  expect(run([A], [[A, [asking]]]).needs).toEqual([]);
  const { snoozed_until: _cleared, ...woken } = asking;
  const t = run([A], [[A, [woken]]]);
  expect(t.needs).toEqual([A.matter]);
  expect(t.rows.get(A.matter)).toMatchObject({ need: "hand", thread: woken });
});

test("a done conversation counts for nothing in Needs you", () => {
  const ready = thread(1, B, {
    seen: false,
    last_stop: { reason: "end_turn", at: NOW },
    done: true,
  });
  expect(run([B], [[B, [ready]]]).needs).toEqual([]);
});

test("a matter whose every conversation not done is snoozed to one time is snoozed whole, unless pinned", () => {
  const a = [thread(1, A, { snoozed_until: LATER }), thread(2, A, { snoozed_until: LATER })];
  const done = thread(3, A, { done: true });
  expect(wholly([...a, done], NOW)).toBe(LATER);
  expect(run([A], [[A, [...a, done]]]).group.get(A.matter)).toBe("snoozed");
  // two times, one awake, none at all, or the time come: not whole
  expect(
    wholly([a[0] as ThreadSummary, thread(2, A, { snoozed_until: LATER + 1 })], NOW),
  ).toBeNull();
  expect(wholly([a[0] as ThreadSummary, thread(2, A)], NOW)).toBeNull();
  expect(wholly([], NOW)).toBeNull();
  expect(wholly(a, LATER)).toBeNull();
  const pinned = matter(1, { pinned: true });
  expect(run([pinned], [[pinned, a]]).group.get(A.matter)).toBe("pinned");
});

test("the shelf: a matter snoozed whole as one row, else each snoozed conversation, the next back first", () => {
  const whole = [thread(1, A, { snoozed_until: LATER + 5 })];
  const one = thread(2, B, { snoozed_until: LATER });
  const gone = thread(4, B, { snoozed_until: NOW - 1 });
  const archived = thread(5, B, { snoozed_until: LATER, status: "archived" });
  const rows = shelf(
    [A, B],
    new Map([
      [A.matter, whole],
      [B.matter, [one, thread(3, B), gone, archived]],
    ]),
    NOW,
  );
  expect(rows.map((r) => [r.matter.matter, r.thread?.thread ?? null, r.until])).toEqual([
    [B.matter, one.thread, LATER],
    [A.matter, null, LATER + 5],
  ]);
});
