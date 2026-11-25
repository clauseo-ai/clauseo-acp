// Whether a plan is being updated (agent-update.ts), from the host's install events alone: stage D's flag on the
// event, or Missing's own installed_before.
import { expect, test } from "vitest";
import type { InstallProgress } from "../transport/protocol.ts";
import { installedBefore, updateOf } from "./agent-update.ts";

const HAD: InstallProgress = { missing: { download_bytes: 200_000_000, installed_before: true } };
const NEW: InstallProgress = { missing: { download_bytes: 200_000_000 } };
const DOWN: InstallProgress = { downloading: { received: 1, total: 2 } };
const READY: InstallProgress = { ready: { version: "0.2.0" } };

const ev = (state: InstallProgress, flag = false) =>
  flag
    ? ({ agent: "codex-acp", state, installed_before: true } as never)
    : { agent: "codex-acp", state };

/** The update's state after each event in turn. */
const run = (...events: ReturnType<typeof ev>[]) => {
  let was: true | undefined;
  return events.map((e) => (was = updateOf(was, e)));
};

test("either flag says the plan was added before", () => {
  expect(installedBefore(ev(DOWN, true))).toBe(true);
  expect(installedBefore(ev(HAD))).toBe(true);
  expect(installedBefore(ev(DOWN))).toBe(false);
  expect(installedBefore(ev(NEW))).toBe(false);
});

test("stage D's update: flagged from the first Downloading to Ready, which ends it", () => {
  expect(run(ev(DOWN, true), ev("installing", true), ev(READY, true))).toEqual([
    true,
    true,
    undefined,
  ]);
});

test("a host before stage D: Missing's flag, then unflagged states keep it until Ready", () => {
  expect(run(ev(HAD), ev(DOWN), ev("paused"), ev(HAD), ev(READY))).toEqual([
    true,
    true,
    true,
    true,
    undefined,
  ]);
});

test("a plan never added, or the firm's, is no update", () => {
  expect(run(ev(NEW), ev(DOWN), ev(READY))).toEqual([undefined, undefined, undefined]);
  expect(run(ev(DOWN, true), ev(NEW))).toEqual([true, undefined]);
  expect(run(ev(HAD), ev("managed", true))).toEqual([true, undefined]);
});
