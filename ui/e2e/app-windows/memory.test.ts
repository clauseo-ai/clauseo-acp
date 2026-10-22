// walkTree and treeMemory on fixed snapshots; no Windows or app needed.

import { expect, test } from "@playwright/test";
import { treeMemory, walkTree, type Row } from "./memory.ts";

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
  expect(treeMemory(tree)).toEqual({ privateBytes: 21_000, privateWorkingSetBytes: 2_100 });
  const missing = walkTree([row(10, 4, 100), { ...row(11, 10, 200), WorkingSetPrivate: null }], 10);
  expect(treeMemory(missing)).toEqual({ privateBytes: 21_000, privateWorkingSetBytes: null });
});
