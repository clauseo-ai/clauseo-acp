// Memory of a process tree on Windows: the app plus every process below it (WebView2's
// msedgewebview2.exe browser, renderer, GPU and utility processes), from one snapshot of
// Win32_Process and of the per-process performance counters.

import { execFile } from "node:child_process";
import { setTimeout as wait } from "node:timers/promises";
import { promisify } from "node:util";

/** One process of the tree. */
export interface TreeProcess {
  pid: number;
  name: string;
  commandLine: string | null;
  /** Win32_Process.PrivatePageCount: committed private bytes (Task Manager's "Commit size"). */
  privateBytes: number;
  /** The WorkingSetPrivate counter (Task Manager's "Memory"); null if the counter is missing. */
  privateWorkingSetBytes: number | null;
}

/** One row of the snapshot, as PowerShell prints it. */
export interface Row {
  ProcessId: number;
  ParentProcessId: number;
  /** Start time in milliseconds, on one clock for all rows; null for the System processes. */
  CreatedMs: number | null;
  Name: string;
  CommandLine: string | null;
  PrivatePageCount: number;
  WorkingSetPrivate: number | null;
}

/** The tree's memory, summed. */
export interface TreeMemory {
  privateBytes: number;
  /** Null if any process lacks the counter. */
  privateWorkingSetBytes: number | null;
}

/** How long PowerShell may take for one snapshot. */
const SNAPSHOT_TIMEOUT_MS = 30_000;

const QUERY = [
  "$ws = @{}",
  "Get-CimInstance Win32_PerfRawData_PerfProc_Process | ForEach-Object { $ws[[string]$_.IDProcess] = $_.WorkingSetPrivate }",
  "Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId," +
    " @{n='CreatedMs';e={ if ($_.CreationDate) { $_.CreationDate.ToFileTimeUtc() / 10000 } }}," +
    " Name, CommandLine, PrivatePageCount," +
    " @{n='WorkingSetPrivate';e={ $ws[[string]$_.ProcessId] }} | ConvertTo-Json -Compress",
].join("; ");

const isNumberOrNull = (value: unknown): boolean => typeof value === "number" || value === null;

function isRow(value: unknown): value is Row {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const row = value as Record<string, unknown>;
  return (
    typeof row["ProcessId"] === "number" &&
    typeof row["ParentProcessId"] === "number" &&
    isNumberOrNull(row["CreatedMs"]) &&
    typeof row["Name"] === "string" &&
    (typeof row["CommandLine"] === "string" || row["CommandLine"] === null) &&
    typeof row["PrivatePageCount"] === "number" &&
    isNumberOrNull(row["WorkingSetPrivate"])
  );
}

/**
 * Every process in the tree rooted at `root`, the root first. Windows does not reparent a process
 * whose parent ends, and it reuses PIDs, so a process that names a tree member as its parent is a
 * child only if it started after that member. Throws if `root` is not in `rows`.
 */
export function walkTree(rows: readonly Row[], root: number): TreeProcess[] {
  const rootRow = rows.find((row) => row.ProcessId === root);
  if (rootRow === undefined) {
    throw new Error(`process ${root} is not running`);
  }
  const tree: TreeProcess[] = [];
  // Two processes started in the same millisecond could name each other; `seen` ends that cycle.
  const seen = new Set<number>();
  const queue = [rootRow];
  for (let parent = queue.shift(); parent !== undefined; parent = queue.shift()) {
    if (seen.has(parent.ProcessId)) {
      continue;
    }
    seen.add(parent.ProcessId);
    tree.push({
      pid: parent.ProcessId,
      name: parent.Name,
      commandLine: parent.CommandLine,
      privateBytes: parent.PrivatePageCount,
      privateWorkingSetBytes: parent.WorkingSetPrivate,
    });
    const startedAt = parent.CreatedMs;
    for (const row of rows) {
      if (
        row.ParentProcessId === parent.ProcessId &&
        row.ProcessId !== parent.ProcessId &&
        startedAt !== null &&
        row.CreatedMs !== null &&
        row.CreatedMs >= startedAt
      ) {
        queue.push(row);
      }
    }
  }
  return tree;
}

/** The sums over `tree`. */
export function treeMemory(tree: readonly TreeProcess[]): TreeMemory {
  let privateWorkingSetBytes: number | null = 0;
  for (const entry of tree) {
    privateWorkingSetBytes =
      privateWorkingSetBytes === null || entry.privateWorkingSetBytes === null
        ? null
        : privateWorkingSetBytes + entry.privateWorkingSetBytes;
  }
  return {
    privateBytes: tree.reduce((sum, entry) => sum + entry.privateBytes, 0),
    privateWorkingSetBytes,
  };
}

/** The tree rooted at `root`, from a fresh snapshot. Throws if `root` is not running. */
export async function processTree(root: number): Promise<TreeProcess[]> {
  const { stdout } = await promisify(execFile)(
    "powershell",
    ["-NoProfile", "-NonInteractive", "-Command", QUERY],
    { encoding: "utf8", timeout: SNAPSHOT_TIMEOUT_MS, maxBuffer: 64 * 1024 * 1024 },
  );
  const parsed: unknown = JSON.parse(stdout);
  if (!Array.isArray(parsed) || !parsed.every(isRow)) {
    throw new Error(`the snapshot has an unexpected shape: ${stdout.slice(0, 200)}`);
  }
  return walkTree(parsed, root);
}

/** How many snapshots in a row `settle` needs to agree. */
const SETTLE_WINDOW = 3;
/** How far apart the window's private bytes may be, as a fraction of the largest. */
const SETTLE_TOLERANCE = 0.01;
/** The pause between two snapshots. */
const SETTLE_INTERVAL_MS = 1_000;

/** How `settle` ended. */
export interface Settled {
  /** False if the private bytes were still moving when the time ran out. */
  settled: boolean;
  ms: number;
}

/**
 * Takes snapshots of the tree until the last `SETTLE_WINDOW` agree on private bytes within
 * `SETTLE_TOLERANCE`, or `maxMs` has passed.
 */
export async function settle(root: number, maxMs: number): Promise<Settled> {
  const start = performance.now();
  const totals: number[] = [];
  for (;;) {
    totals.push(treeMemory(await processTree(root)).privateBytes);
    const window = totals.slice(-SETTLE_WINDOW);
    const settled =
      window.length === SETTLE_WINDOW &&
      Math.max(...window) - Math.min(...window) <= SETTLE_TOLERANCE * Math.max(...window);
    const ms = performance.now() - start;
    if (settled || ms > maxMs) {
      return { settled, ms: Math.round(ms) };
    }
    await wait(SETTLE_INTERVAL_MS);
  }
}
