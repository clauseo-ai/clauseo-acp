// Private memory of a process tree on Windows: the app plus every process below it (WebView2's
// msedgewebview2.exe browser, renderer, GPU and utility processes), from one Win32_Process snapshot.

import { execFileSync } from "node:child_process";

/** One process of the tree. */
export interface TreeProcess {
  pid: number;
  name: string;
  /** Win32_Process.PrivatePageCount: committed private bytes (Task Manager's "Commit size"). */
  privateBytes: number;
}

interface Row {
  ProcessId: number;
  ParentProcessId: number;
  Name: string;
  PrivatePageCount: number;
}

const QUERY =
  "Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, PrivatePageCount | ConvertTo-Json -Compress";

function isRow(value: unknown): value is Row {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const row = value as Record<string, unknown>;
  return (
    typeof row["ProcessId"] === "number" &&
    typeof row["ParentProcessId"] === "number" &&
    typeof row["Name"] === "string" &&
    typeof row["PrivatePageCount"] === "number"
  );
}

/** Every process in the tree rooted at `root`, the root first. Throws if `root` is not running. */
export function processTree(root: number): TreeProcess[] {
  const out = execFileSync("powershell", ["-NoProfile", "-NonInteractive", "-Command", QUERY], {
    encoding: "utf8",
  });
  const parsed: unknown = JSON.parse(out);
  if (!Array.isArray(parsed) || !parsed.every(isRow)) {
    throw new Error(`Win32_Process returned an unexpected shape: ${out.slice(0, 200)}`);
  }
  const rows: Row[] = parsed;
  const byPid = new Map(rows.map((row) => [row.ProcessId, row]));
  if (!byPid.has(root)) {
    throw new Error(`process ${root} is not running`);
  }
  // A PID is reused only after its process ends, but a dead parent's PID can come back as an
  // unrelated process, so each PID is visited once.
  const tree: TreeProcess[] = [];
  const seen = new Set<number>();
  const queue = [root];
  for (let pid = queue.shift(); pid !== undefined; pid = queue.shift()) {
    const row = byPid.get(pid);
    if (row === undefined || seen.has(pid)) {
      continue;
    }
    seen.add(pid);
    tree.push({ pid, name: row.Name, privateBytes: row.PrivatePageCount });
    for (const child of rows) {
      if (child.ParentProcessId === pid && child.ProcessId !== pid) {
        queue.push(child.ProcessId);
      }
    }
  }
  return tree;
}
