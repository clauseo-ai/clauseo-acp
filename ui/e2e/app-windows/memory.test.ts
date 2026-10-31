// walkTree and treeMemory on fixed snapshots, and settle's rule on fixed readings; no Windows or
// app needed.

import { expect, test } from "@playwright/test";
import { isSettled, parseCommit, treeMemory, walkTree, type Row } from "./memory.ts";

function row(
  pid: number,
  parent: number,
  createdMs: number | null,
  name = "msedgewebview2.exe",
): Row {
  return {
    ProcessId: pid,
    ParentProcessId: parent,
    CreatedMs: createdMs,
    Name: name,
    CommandLine: null,
    PrivatePageCount: pid * 1000,
    WorkingSetPrivate: pid * 100,
  };
}

const pids = (rows: readonly Row[], root: number): number[] =>
  walkTree(rows, root).map((entry) => entry.pid);

test("walks every generation below the root, the root first", () => {
  const rows = [
    row(4, 0, null, "System"),
    row(10, 4, 100, "clauseo-desktop.exe"),
    row(11, 10, 200),
    row(12, 11, 300),
    row(13, 11, 300),
  ];
  expect(pids(rows, 10)).toEqual([10, 11, 12, 13]);
});

test("leaves out a process whose parent PID was reused by a tree member", () => {
  // 20 started at 50 under an earlier 11 that ended; the app's child 11 took the PID at 200.
  const rows = [row(10, 4, 100), row(11, 10, 200), row(20, 11, 50), row(21, 20, 60)];
  expect(pids(rows, 10)).toEqual([10, 11]);
});

test("leaves out a process with no start time", () => {
  expect(pids([row(10, 4, 100), row(11, 10, null)], 10)).toEqual([10]);
});

test("ends a cycle of processes started in the same millisecond", () => {
  expect(pids([row(10, 11, 100), row(11, 10, 100)], 10)).toEqual([10, 11]);
});

test("throws when the root is not running", () => {
  expect(() => walkTree([row(10, 4, 100)], 99)).toThrow("process 99 is not running");
});

test("sums the tree, and has no working set total if one process lacks the counter", () => {
  const tree = walkTree([row(10, 4, 100), row(11, 10, 200)], 10);
  expect(treeMemory(tree)).toEqual({ privateWorkingSetBytes: 2_100, commitBytes: 21_000 });
  const missing = walkTree([row(10, 4, 100), { ...row(11, 10, 200), WorkingSetPrivate: null }], 10);
  expect(treeMemory(missing)).toEqual({ privateWorkingSetBytes: null, commitBytes: 21_000 });
});

test("settles when the last three readings agree within 1%", () => {
  // The shape seen on windows-11-arm: a first reading a few MiB high, then two that agree.
  expect(isSettled([120_000_000, 100_000_000, 100_500_000, 100_990_000])).toBe(true);
  expect(isSettled([100_000_000, 100_000_000, 100_000_000])).toBe(true);
});

test("does not settle with fewer than three readings, or while the commit moves", () => {
  expect(isSettled([])).toBe(false);
  expect(isSettled([100_000_000, 100_000_000])).toBe(false);
  // 100 to 101.1 MB is just over 1% of the largest.
  expect(isSettled([100_000_000, 100_500_000, 101_100_000])).toBe(false);
  // Settled earlier, but the last three spread.
  expect(isSettled([100_000_000, 100_000_000, 100_000_000, 110_000_000])).toBe(false);
});

test("reads a commit total from PowerShell's line, and null from an empty or other line", () => {
  expect(parseCommit("104857600")).toBe(104_857_600);
  expect(parseCommit(" 104857600\r")).toBe(104_857_600);
  expect(parseCommit("")).toBeNull();
  expect(parseCommit("  ")).toBeNull();
  expect(parseCommit("Get-Process : Cannot find a process")).toBeNull();
});
