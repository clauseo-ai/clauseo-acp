// The WebView2 processes still running on a test's scratch folder after the app ended, for
// scratch.ts's removeScratch. Under load they outlive the app by tens of seconds to minutes and
// hold the profile, so the folder cannot be removed (clauseo-acp-private
// measurements/pr340-e2e-loop). They are found by command line, whose `--user-data-dir` names the
// folder, not by parent: a `taskkill /T` that could not end a child has cut it off from the tree.
// The script and the parsing run on any OS (holders.test.ts); finding and killing need Windows.

import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { powershellScript } from "./memory.ts";
import type { Holders } from "./scratch.ts";

/** WebView2's process image. */
export const WEBVIEW_IMAGE = "msedgewebview2.exe";

/** How long `taskkill` may take. */
const KILL_TIMEOUT_MS = 30_000;

/** The PowerShell that prints every `image` process's PID and command line, as a JSON array. */
export function holdersScript(image = WEBVIEW_IMAGE): string {
  if (!/^[\w.-]+$/.test(image)) {
    throw new Error(`not a process image name: ${JSON.stringify(image)}`);
  }
  return `$ErrorActionPreference = 'Stop'
$found = @(Get-CimInstance Win32_Process -Filter "Name='${image}'" | Select-Object ProcessId, CommandLine)
ConvertTo-Json -Compress -InputObject $found
`;
}

/** One row of the script's output. */
interface Row {
  ProcessId: number;
  CommandLine: string | null;
}

function isRow(value: unknown): value is Row {
  if (typeof value !== "object" || value === null) return false;
  const row = value as Record<string, unknown>;
  return (
    Number.isSafeInteger(row["ProcessId"]) &&
    (typeof row["CommandLine"] === "string" || row["CommandLine"] === null)
  );
}

/**
 * The PIDs in the script's output whose command line names `root` or a path below it, without
 * case, as Windows compares paths. Throws on output that is not the script's.
 */
export function parseHolders(stdout: string, root: string): number[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(stdout);
  } catch {
    throw new Error(`the holders script printed ${JSON.stringify(stdout.slice(0, 200))}, not JSON`);
  }
  if (!Array.isArray(parsed) || !parsed.every(isRow)) {
    throw new Error(`the holders script printed an unexpected shape: ${stdout.slice(0, 200)}`);
  }
  const wanted = root.replace(/[\\/]+$/, "").toLowerCase();
  return parsed
    .filter((row) => row.CommandLine !== null && names(row.CommandLine.toLowerCase(), wanted))
    .map((row) => row.ProcessId);
}

/** Whether `commandLine` holds `root` followed by a separator, a quote, a space or its end. */
function names(commandLine: string, root: string): boolean {
  for (let at = commandLine.indexOf(root); at !== -1; at = commandLine.indexOf(root, at + 1)) {
    const next = commandLine.charAt(at + root.length);
    if (next === "" || next === "\\" || next === "/" || next === '"' || next === " ") return true;
  }
  return false;
}

/** The running `image` processes whose command line names `root`. */
export async function findHolders(root: string, image = WEBVIEW_IMAGE): Promise<number[]> {
  return parseHolders(await powershellScript(holdersScript(image)), root);
}

/**
 * Ends `pids` and their trees. Its failures are not reported: a process already gone fails it
 * too, and the caller asks again afterwards which are left.
 */
export async function killHolders(pids: readonly number[]): Promise<void> {
  if (pids.length === 0) return;
  await promisify(execFile)(
    "taskkill",
    ["/T", "/F", ...pids.flatMap((pid) => ["/PID", String(pid)])],
    { timeout: KILL_TIMEOUT_MS },
  ).catch(() => {});
}

/** The WebView2 processes on a scratch folder, for removeScratch. */
export const webviewHolders: Holders = {
  find: (root) => findHolders(root),
  kill: killHolders,
};
