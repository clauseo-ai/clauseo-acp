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
  /**
   * The WorkingSetPrivate counter (Task Manager's "Memory"); null if the snapshot did not ask for
   * it or the counter is missing.
   */
  privateWorkingSetBytes: number | null;
  /** Win32_Process.PrivatePageCount: committed private bytes (Task Manager's "Commit size"). */
  commitBytes: number;
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
  /** Null if any process lacks the counter. */
  privateWorkingSetBytes: number | null;
  commitBytes: number;
}

/**
 * How long one PowerShell call may take. WMI is slow on windows-11-arm: a snapshot takes 10 to 20 s
 * there, and the first one on a fresh runner took over 30 s.
 */
const POWERSHELL_TIMEOUT_MS = 90_000;

/** Fills `$ws` with each PID's private working set. */
const WORKING_SETS =
  "Get-CimInstance Win32_PerfRawData_PerfProc_Process | ForEach-Object { $ws[[string]$_.IDProcess] = $_.WorkingSetPrivate }";

const PROCESSES =
  "Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId," +
  " @{n='CreatedMs';e={ if ($_.CreationDate) { $_.CreationDate.ToFileTimeUtc() / 10000 } }}," +
  " Name, CommandLine, PrivatePageCount," +
  " @{n='WorkingSetPrivate';e={ $ws[[string]$_.ProcessId] }} | ConvertTo-Json -Compress";

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
      privateWorkingSetBytes: parent.WorkingSetPrivate,
      commitBytes: parent.PrivatePageCount,
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
    privateWorkingSetBytes,
    commitBytes: tree.reduce((sum, entry) => sum + entry.commitBytes, 0),
  };
}

/** What `command` prints. */
async function powershell(command: string): Promise<string> {
  const { stdout } = await promisify(execFile)(
    "powershell",
    ["-NoProfile", "-NonInteractive", "-Command", command],
    { encoding: "utf8", timeout: POWERSHELL_TIMEOUT_MS, maxBuffer: 64 * 1024 * 1024 },
  );
  return stdout;
}

/**
 * The tree rooted at `root`, from a fresh snapshot, with each process's private working set if
 * `workingSet` is set (the counters add seconds). Throws if `root` is not running.
 */
export async function processTree(
  root: number,
  { workingSet = false }: { workingSet?: boolean } = {},
): Promise<TreeProcess[]> {
  const stdout = await powershell(
    ["$ws = @{}", ...(workingSet ? [WORKING_SETS] : []), PROCESSES].join("; "),
  );
  const parsed: unknown = JSON.parse(stdout);
  if (!Array.isArray(parsed) || !parsed.every(isRow)) {
    throw new Error(`the snapshot has an unexpected shape: ${stdout.slice(0, 200)}`);
  }
  return walkTree(parsed, root);
}

/** How many readings in a row `settle` needs to agree. */
const SETTLE_WINDOW = 3;
/** How far apart the window's readings may be, as a fraction of the largest. */
const SETTLE_TOLERANCE = 0.01;
/** The pause between two readings. */
const SETTLE_INTERVAL_MS = 1_000;

/** One reading of `settle`. */
export interface Reading {
  /** Since `settle` started, when the reading came back. */
  ms: number;
  commitBytes: number;
}

/** How `settle` ended. */
export interface Settled {
  /** False if the commit was still moving when the time ran out. */
  settled: boolean;
  ms: number;
  /** Every reading, so a run that never settles shows whether the counter drifts or is noisy. */
  readings: Reading[];
}

/**
 * Reads the commit of `pids` until the last `SETTLE_WINDOW` readings agree within
 * `SETTLE_TOLERANCE`, or `maxMs` has passed. It reads `Get-Process`, the same counter as
 * `PrivatePageCount` without WMI's cost; a process that has ended counts as nothing.
 */
export async function settle(pids: readonly number[], maxMs: number): Promise<Settled> {
  const query = `(Get-Process -Id ${pids.join(",")} -ErrorAction SilentlyContinue | Measure-Object PrivateMemorySize64 -Sum).Sum`;
  const start = performance.now();
  const readings: Reading[] = [];
  for (;;) {
    const out = (await powershell(query)).trim();
    const total = Number(out);
    if (out === "" || !Number.isFinite(total)) {
      throw new Error(
        `none of processes ${pids.join(", ")} is running, or Get-Process printed ${out}`,
      );
    }
    const ms = Math.round(performance.now() - start);
    readings.push({ ms, commitBytes: total });
    const window = readings.slice(-SETTLE_WINDOW).map((reading) => reading.commitBytes);
    const settled =
      window.length === SETTLE_WINDOW &&
      Math.max(...window) - Math.min(...window) <= SETTLE_TOLERANCE * Math.max(...window);
    if (settled || ms > maxMs) {
      return { settled, ms, readings };
    }
    await wait(SETTLE_INTERVAL_MS);
  }
}
