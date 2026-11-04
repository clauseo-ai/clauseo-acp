// triage.ts, pure: each ThreadStatus and last stop in its group as the approved page's table has it, a pinned matter
// staying in Pinned, held rows keeping their group, the thread behind each row, and Needs you's order (the hand's waits,
// then answers ready, then failures, each in the order it came).
import { expect, test } from "vitest";
import type { ThreadStatus, ThreadSummary } from "../transport/protocol.ts";
import { uuid } from "../testing/stream-script.ts";
import { detailed } from "./fake-next.ts";
import type { MatterInfo, MatterStatus } from "./host-next.ts";
import type { RowStatus } from "./matters.ts";
import { rowOf } from "./matters.ts";
import { needOf, triage, type Arrival, type Group } from "./triage.ts";

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
    ["a failure, unseen", stop("failed"), "idle", "needs", "failed"],
    ["a failure, seen", stop("failed", true), "idle", "date", undefined],
    ["working", { status: "running" }, "working", "date", "working"],
    ["starting", { status: "starting" }, "working", "date", "working"],
    ["queued", { status: { queued: { behind: uuid(99, 3) } } }, "working", "date", "working"],
    [
      "a limit pause, until U14",
      { status: { paused_at_limit: { resets_at: NOW + 3_600_000 } } },
      { paused_at_limit: { resets_at: NOW + 3_600_000 } },
      "date",
      "limit",
    ],
    ["stopped at the limit", stop("at_limit"), "idle", "date", undefined],
    ["cancelled", stop("cancelled"), { stopped: { at: NOW - 60_000 } }, "date", "stopped"],
    ["read, nothing waiting", stop("end_turn", true), "idle", "date", undefined],
    ["archived, unread", { status: "archived", ...stop("end_turn") }, "idle", "date", undefined],
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

test("a matter's row names the thread behind it: the oldest waiting, over an answer ready", () => {
  const m = matter(1, { status: "needs_answer" });
  const ready = thread(1, m, { seen: false, last_stop: { reason: "end_turn", at: NOW } });
  const newer = thread(2, m, { status: "needs_you", last_activity: NOW - 1000, title: "Newer" });
  const older = thread(3, m, { status: "needs_you", last_activity: NOW - 9000, title: "Older" });
  const t = run([m], [[m, [ready, newer, older]]]);
  expect(t.rows.get(m.matter)?.thread?.title).toBe("Older");
  expect(t.rows.get(m.matter)?.need).toBe("hand");
});

test("a pinned matter that needs the lawyer stays in Pinned, with its line and thread", () => {
  const m = matter(1, { pinned: true, status: "needs_permission" });
  const t = run([m], [[m, [thread(1, m, { status: "needs_you", title: "Rejoinder" })]]]);
  expect(t.group.get(m.matter)).toBe("pinned");
  expect(t.needs).toEqual([]);
  expect(t.rows.get(m.matter)?.status?.state).toBe("permission");
  expect(t.rows.get(m.matter)?.thread?.title).toBe("Rejoinder");
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
    [read.matter, "date"],
  ]);
  const t = run([m, read], lists, held);
  expect(t.group.get(m.matter)).toBe("needs");
  expect(t.group.get(read.matter)).toBe("date");
  // its words still change: the held row in Needs you says nothing now
  expect(t.rows.get(m.matter)?.status).toBeUndefined();
});

test("Needs you: the hand's waits, then answers ready, then failures, each in the order it came", () => {
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
  expect(first.needs).toEqual([c.matter, b.matter, a.matter]);
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
  expect(second.needs).toEqual([c.matter, d.matter, b.matter, e.matter, a.matter]);
});

test("without its threads, a matter's own status places it: a wait or an answer ready need the lawyer", () => {
  const ask = matter(1, { status: "needs_answer" });
  const busy = matter(2, { status: "working" });
  const t = run([ask, busy], []);
  expect(t.group.get(ask.matter)).toBe("needs");
  expect(t.rows.get(ask.matter)?.thread).toBeNull();
  expect(t.group.get(busy.matter)).toBe("date");
});
