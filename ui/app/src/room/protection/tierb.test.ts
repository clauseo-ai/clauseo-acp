// R17 PR-E's causes, typed: each cause's name and group (pool_owned joins can't, with blocked and other_account), the
// pool's why and needs_reset, which pool offers Add room or Free up places, the note's why, and set_up_at. The
// thread's side: a step's stop line for left_safe_workspace and a failure naming where, and which breaks refill.
import { expect, test } from "vitest";
import { step } from "../../testing/stream-script.ts";
import type { NotProtectedCause, StepEntry } from "../../transport/protocol.ts";
import { ASKS_AGAIN, refillOf, RESTARTS, type Break } from "../thread-break.ts";
import { stopLine } from "../thread/stops.ts";
import { blockOf, needsInstaller, repairByInstaller } from "./blocked.ts";
import { causeOf, notProtectedCause } from "./causes.ts";
import { poolOf } from "./detail.ts";
import { noteWhy } from "./note-why.ts";
import { canFreeUp, canRetry, setUpAt } from "./view.ts";
import { np, view } from "./fixtures.ts";

const pool = (over: object = {}): NotProtectedCause =>
  ({
    pool_full: { size: 8, open: 5, held: 0, owed: 2, set_aside: 1, ...over },
  }) as NotProtectedCause;

test.each([
  ["pool_owned", "pool_owned", "cant"],
  ["other_account", "other_account", "cant"],
  [{ blocked: { reason: "policy" } }, "blocked", "cant"],
  [pool(), "pool_full", "pool"],
  ["needs_set_up", "needs_set_up", "setup"],
  ["set_up_refused", "set_up_refused", "setup"],
  ["set_up_failed", "set_up_failed", "repair"],
  ["unsupported", "unsupported", "cant"],
  ["a_cause_from_later", "unknown", "cant"],
] as const)("%j is %s, in the %s group", (raw, name, group) => {
  const cause = causeOf(raw as NotProtectedCause);
  expect(cause).toMatchObject({ name, group });
  expect(notProtectedCause(np(raw))).toMatchObject({ name, group });
});

test("pool_owned offers no Try again, and its note is its own", () => {
  const owned = causeOf("pool_owned");
  expect(canRetry(owned)).toBe(false);
  expect(canFreeUp(owned)).toBe(false);
  expect(noteWhy(owned)).toBe("pool_owned");
  expect(blockOf(owned)).toBeNull();
});

test("blocked and other_account keep blocked.ts's lines through the generated type", () => {
  expect(blockOf(causeOf({ blocked: { reason: "logon_right" } }))).toBe("logon_right");
  expect(blockOf(causeOf({ blocked: { reason: "other_account" } }))).toBe("unknown");
  expect(blockOf(causeOf({ blocked: { reason: "later" } }))).toBe("unknown");
  expect(blockOf(causeOf("other_account"))).toBe("other_account");
  expect(noteWhy(causeOf("other_account"))).toBe("blocked");
});

test.each([
  [{}, null, 0],
  [{ why: "no_free_slot", needs_reset: 2 }, "no_free_slot", 2],
  [{ why: "matter_sessions_full" }, "matter_sessions_full", 0],
  [{ why: "unknown" }, null, 0],
  [{ why: "a_why_from_later", needs_reset: -1 }, null, 0],
])("the pool's why and needs_reset from %j", (over, why, resets) => {
  expect(poolOf(causeOf(pool(over)))).toMatchObject({ size: 8, why, needs_reset: resets });
});

test("Add room under 32 and Free up places past 0 resets, neither for two conversations in the matter", () => {
  expect(canRetry(causeOf(pool()))).toBe(true);
  expect(canRetry(causeOf(pool({ size: 32 })))).toBe(false);
  expect(canRetry(causeOf(pool({ why: "matter_sessions_full" })))).toBe(false);
  expect(canFreeUp(causeOf(pool()))).toBe(false);
  expect(canFreeUp(causeOf(pool({ needs_reset: 1 })))).toBe(true);
  expect(canFreeUp(causeOf(pool({ size: 32, needs_reset: 4 })))).toBe(true);
  expect(canFreeUp(causeOf(pool({ why: "matter_sessions_full", needs_reset: 4 })))).toBe(false);
});

test("set_up_at, typed: a time when the view has one", () => {
  const computer = np("needs_set_up");
  expect(setUpAt(view(computer))).toBeNull();
  expect(setUpAt(view(computer, { set_up_at: 1_762_400_000_000 }))).toBe(1_762_400_000_000);
  expect(setUpAt(null)).toBeNull();
});

const stepOf = (over: Partial<StepEntry>): StepEntry => ({ ...step(1).step!, ...over });

test("left_safe_workspace has its own stop line; a failure carries the item it named", () => {
  expect(stopLine(stepOf({ stop: "left_safe_workspace" }))).toEqual({
    kind: "left_safe_workspace",
  });
  expect(
    stopLine(stepOf({ stop: "failed", failure: "sandbox.pool_owned", failure_at: "Pleadings" })),
  ).toEqual({ kind: "error", code: "sandbox.pool_owned", at: "Pleadings" });
  expect(stopLine(stepOf({ stop: "failed", failure: "sandbox.cant_label" }))).toEqual({
    kind: "error",
    code: "sandbox.cant_label",
  });
});

const brk = (code: string): Break => ({
  id: "step:x",
  step: stepOf({ stop: "failed", failure: code as never }),
  code,
  question: "Q",
  seen: 0,
});

test("a full pool refills the question with or without a restart; pool_owned and cant_label never do", () => {
  expect([...ASKS_AGAIN]).toEqual(["sandbox.pool_full"]);
  const restart = { restartAgent: () => Promise.resolve() } as never;
  expect(refillOf(brk("sandbox.pool_full"), true, null)).toEqual({ key: "step:x", text: "Q" });
  expect(refillOf(brk("sandbox.pool_full"), true, restart)).toEqual({ key: "step:x", text: "Q" });
  expect(refillOf(brk("sandbox.pool_full"), false, null)).toBeNull();
  for (const code of ["sandbox.pool_owned", "sandbox.cant_label"]) {
    expect(RESTARTS.has(code)).toBe(false);
    expect(refillOf(brk(code), true, null)).toBeNull();
  }
});

test("M4: set_up_failed offers no retry, its note is its own, and its repair is the installer's (needsInstaller unchanged)", () => {
  const failed = causeOf("set_up_failed");
  expect(canRetry(failed)).toBe(false);
  expect(noteWhy(failed)).toBe("set_up_failed");
  expect(repairByInstaller(failed)).toBe(true);
  expect(needsInstaller(failed)).toBe(false);
  const missing = causeOf({ blocked: { reason: "admin_copy_missing" } });
  expect(repairByInstaller(missing)).toBe(true);
  expect(needsInstaller(missing)).toBe(true);
  for (const raw of ["needs_set_up", "set_up_refused", "unsupported", "pool_owned"] as const)
    expect(repairByInstaller(causeOf(raw))).toBe(false);
  // the set-up group keeps Try again; a cause from later has its own note and none
  expect(canRetry(causeOf("needs_set_up"))).toBe(true);
  expect(canRetry(causeOf("set_up_refused"))).toBe(true);
  const later = causeOf("a_cause_from_later");
  expect(noteWhy(later)).toBe("unknown");
  expect(canRetry(later)).toBe(false);
  expect(noteWhy(causeOf("unsupported"))).toBe("cant");
});

test("#722's probe_failed: the set-up group, its own note, Try again offered, no installer repair", () => {
  const probe = causeOf("probe_failed");
  expect(probe.name).toBe("probe_failed");
  expect(probe.group).toBe("setup");
  expect(noteWhy(probe)).toBe("probe_failed");
  expect(canRetry(probe)).toBe(true);
  expect(repairByInstaller(probe)).toBe(false);
});
