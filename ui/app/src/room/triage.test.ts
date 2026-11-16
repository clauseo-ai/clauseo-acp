// triage.ts, pure: each ThreadStatus and last stop in its group as the approved page's table has it, a pinned matter
// in Needs you or Working while it needs you or answers, and in Pinned after, held rows keeping their group, the thread behind each row, and Needs you's order (the hand's waits,
// then answers ready, then failures, each in the order it came).
import { expect, test } from "vitest";
import type { ThreadStatus, ThreadSummary } from "../transport/protocol.ts";
import { uuid } from "../testing/stream-script.ts";
import { detailed } from "./fake-next.ts";
import type { MatterInfo, MatterStatus } from "./host-next.ts";
import type { RowStatus } from "./matters.ts";
import { rowOf } from "./matters.ts";
import { activityOrder, needOf, triage, type Arrival, type Group } from "./triage.ts";

const NOW = Date.UTC(2026, 10, 3, 17, 0);
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
const statusOf = (ms: readonly MatterInfo[]) => {
  const map = new Map<string, RowStatus>();
  for (const m of ms) {
    const s = rowOf(m);
    if (s !== null) map.set(m.matter, s);
  }
  return map;
};
const run = (
  ms: readonly MatterInfo[],
  lists: [MatterInfo, ThreadSummary[]][],
  held: ReadonlyMap<string, Group> | null = null,
  arrived: ReadonlyMap<string, Arrival> = new Map(),
) =>
  triage({
    matters: ms,
    status: statusOf(ms),
    lists: new Map(lists.map(([m, ts]) => [m.matter, ts])),
    held,
    arrived,
    now: NOW,
  });

test("each thread status and last stop lands in its group, as the page's table has it", () => {
  const stop = (reason: NonNullable<ThreadSummary["last_stop"]>["reason"], seen = false) => ({
    seen,
    last_stop: { reason, at: NOW - 60_000 },
  });
  const cases: [string, Partial<ThreadSummary>, MatterStatus, Group, string | undefined][] = [
    ["a question", { status: "needs_you" }, "needs_answer", "needs", "question"],
    ["a permission", { status: "needs_you" }, "needs_permission", "needs", "permission"],
    ["an answer ready, unread", stop("end_turn"), "ready", "needs", "ready"],
    // D-037: a failure is no answer waiting: it says so, with the rest
    ["a failure, unseen", stop("failed"), "idle", "rest", "failed"],
    ["a failure, seen", stop("failed", true), "idle", "rest", undefined],
    ["working", { status: "running" }, "working", "working", "working"],
    ["starting", { status: "starting" }, "working", "working", "working"],
    ["queued", { status: { queued: { behind: uuid(99, 3) } } }, "working", "working", "working"],
    [
      "a limit pause, until U14",
      { status: { paused_at_limit: { resets_at: NOW + 3_600_000 } } },
      { paused_at_limit: { resets_at: NOW + 3_600_000 } },
      "rest",
      "limit",
    ],
    ["stopped at the limit", stop("at_limit"), "idle", "rest", undefined],
    ["cancelled", stop("cancelled"), { stopped: { at: NOW - 60_000 } }, "rest", "stopped"],
    ["read, nothing waiting", stop("end_turn", true), "idle", "rest", undefined],
    ["archived, unread", { status: "archived", ...stop("end_turn") }, "idle", "rest", undefined],
  ];
  for (const [name, over, status, group, state] of cases) {
    const m = matter(1, { status });
    const t = run([m], [[m, [thread(1, m, over)]]]);
    expect(t.group.get(m.matter), name).toBe(group);
    expect(t.rows.get(m.matter)?.status?.state, name).toBe(state);
  }
});

test("a failure's row says when it failed; needOf reads only the table's waits", () => {
  const m = matter(1);
  const t = run(
    [m],
    [[m, [thread(1, m, { seen: false, last_stop: { reason: "failed", at: 42 } })]]],
  );
  expect(t.rows.get(m.matter)?.status).toEqual({ state: "failed", at: 42 });
  const statuses: ThreadStatus[] = ["agent_exited", "idle_stopped"];
  for (const status of statuses)
    expect(needOf({ status, seen: false, last_stop: { reason: "failed", at: 1 } })).toBe("failed");
  expect(needOf({ status: "running", seen: false, last_stop: { reason: "failed", at: 1 } })).toBe(
    null,
  );
});

test("a turn ended on the lawyer's refusal is no answer ready, as a Stop is not", () => {
  for (const reason of ["cancelled", "not_allowed", "outside_matter"] as const)
    expect(needOf({ status: "idle", seen: false, last_stop: { reason, at: 1 } })).toBe(null);
  expect(needOf({ status: "idle", seen: false, last_stop: { reason: "end_turn", at: 1 } })).toBe(
    "ready",
  );
  // a turn too long for the model to read is the model's stop, as a refusal is: ready to read, never failed
  for (const reason of ["refusal", "too_long"] as const)
    expect(needOf({ status: "idle", seen: false, last_stop: { reason, at: 1 } })).toBe("ready");
});

test("a matter's row names the thread behind it: the oldest waiting, over an answer ready", () => {
  const m = matter(1, { status: "needs_answer" });
  const ready = thread(1, m, { seen: false, last_stop: { reason: "end_turn", at: NOW } });
  const newer = thread(2, m, { status: "needs_you", last_activity: NOW - 1000, title: "Newer" });
  const older = thread(3, m, { status: "needs_you", last_activity: NOW - 9000, title: "Older" });
  const t = run([m], [[m, [ready, newer, older]]]);
  expect(t.rows.get(m.matter)?.thread?.title).toBe("Older");
  expect(t.rows.get(m.matter)?.need).toBe("hand");
});

test("a pinned matter stands in Needs you while it needs the lawyer, in Working while it answers, then in Pinned", () => {
  const m = matter(1, { pinned: true, status: "needs_permission" });
  const t = run([m], [[m, [thread(1, m, { status: "needs_you", title: "Rejoinder" })]]]);
  expect(t.group.get(m.matter)).toBe("needs");
  expect(t.needs).toEqual([m.matter]);
  expect(t.rows.get(m.matter)?.status?.state).toBe("permission");
  expect(t.rows.get(m.matter)?.thread?.title).toBe("Rejoinder");
  const busy = matter(1, { pinned: true, status: "working" });
  expect(
    run([busy], [[busy, [thread(1, busy, { status: "running" })]]]).group.get(busy.matter),
  ).toBe("working");
  const after = matter(1, { pinned: true, status: "idle" });
  const done = run([after], [[after, [thread(1, after)]]]);
  expect(done.group.get(after.matter)).toBe("pinned");
  expect(done.needs).toEqual([]);
});

test("a held row keeps its group: the open matter, and every row under the pointer", () => {
  const m = matter(1, { status: "needs_answer" });
  const read = matter(2);
  const lists: [MatterInfo, ThreadSummary[]][] = [
    [m, [thread(1, m)]],
    [read, [thread(2, read, { status: "needs_you" })]],
  ];
  const held = new Map<string, Group>([
    [m.matter, "needs"],
    [read.matter, "rest"],
  ]);
  const t = run([m, read], lists, held);
  expect(t.group.get(m.matter)).toBe("needs");
  expect(t.group.get(read.matter)).toBe("rest");
  // its words still change: the held row in Needs you says nothing now
  expect(t.rows.get(m.matter)?.status).toBeUndefined();
});

test("Needs you: the hand's waits, then answers ready, each in the order it came; a failure stays out (D-037)", () => {
  const [a, b, c, d, e] = [1, 2, 3, 4, 5].map((n) => matter(n));
  if (a === undefined || b === undefined || c === undefined || d === undefined || e === undefined)
    throw new Error("five matters");
  const failed = (m: MatterInfo, n: number) =>
    thread(n, m, { seen: false, last_stop: { reason: "failed", at: NOW - n } });
  const ready = (m: MatterInfo, n: number) =>
    thread(n, m, { seen: false, last_stop: { reason: "end_turn", at: NOW - n } });
  const first = run(
    [a, b, c],
    [
      [a, [failed(a, 1)]],
      [b, [ready(b, 2)]],
      [c, [thread(3, c, { status: "needs_you" })]],
    ],
  );
  expect(first.needs).toEqual([c.matter, b.matter]);
  expect(first.group.get(a.matter)).toBe("rest");
  // d's question comes later: it joins the hand's waits at their foot, after c; e's answer after b
  const second = run(
    [a, b, c, d, e],
    [
      [a, [failed(a, 1)]],
      [b, [ready(b, 2)]],
      [c, [thread(3, c, { status: "needs_you" })]],
      [d, [thread(4, d, { status: "needs_you", last_activity: NOW - 99_000 })]],
      [e, [ready(e, 99_000)]],
    ],
    null,
    first.arrived,
  );
  expect(second.needs).toEqual([c.matter, d.matter, b.matter, e.matter]);
});

test("D-037: nothing waiting, nothing in Needs you: a read answer, a stop, the limit, an idle or archived thread", () => {
  const rows: [string, Partial<ThreadSummary>][] = [
    ["a read answer", { seen: true, last_stop: { reason: "end_turn", at: NOW } }],
    ["a stop the lawyer made", { seen: false, last_stop: { reason: "cancelled", at: NOW } }],
    ["the limit", { seen: false, last_stop: { reason: "at_limit", at: NOW } }],
    ["not allowed", { seen: false, last_stop: { reason: "not_allowed", at: NOW } }],
    ["idle with no stop", { seen: false }],
    [
      "archived, unread",
      { status: "archived", seen: false, last_stop: { reason: "end_turn", at: NOW } },
    ],
    ["a seen mark not known", { last_stop: { reason: "end_turn", at: NOW } }],
  ];
  for (const [name, over] of rows) {
    const m = matter(1, { status: "idle" });
    const t = run([m], [[m, [thread(1, m, over)]]]);
    expect(t.needs, name).toEqual([]);
    expect(t.group.get(m.matter), name).toBe("rest");
  }
});

test("without its threads, a matter's own status places it: a wait or an answer ready need the lawyer", () => {
  const ask = matter(1, { status: "needs_answer" });
  const busy = matter(2, { status: "working" });
  const t = run([ask, busy], []);
  expect(t.group.get(ask.matter)).toBe("needs");
  expect(t.rows.get(ask.matter)?.thread).toBeNull();
  expect(t.group.get(busy.matter)).toBe("working");
});

test("Working's order and the rest's: the latest activity first, then the newest", () => {
  const old = matter(1, { last_activity: NOW - 3_600_000 });
  const fresh = matter(2, { last_activity: NOW - 60_000 });
  const none = matter(3);
  const newer = matter(4);
  expect([none, old, newer, fresh].sort(activityOrder).map((m) => m.matter)).toEqual([
    fresh.matter,
    old.matter,
    newer.matter,
    none.matter,
  ]);
});
