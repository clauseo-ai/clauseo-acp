// Closing the app as a user would: WM_CLOSE posted straight to its main window. `taskkill` without
// /F cannot do it on CI: it only closes visible top-level windows, and in session 0 (services,
// GitHub's runners) no window is visible, so it fails with "can only be terminated forcefully".
// The window is found by its class, `Tauri Window`, among the top-level windows the app's PID owns.

import { powershellScript } from "./memory.ts";

/** The window class tao gives a Tauri window. */
export const WINDOW_CLASS = "Tauri Window";

/** WM_CLOSE. */
const WM_CLOSE = 0x0010;

/**
 * The PowerShell that posts WM_CLOSE to every `Tauri Window` top-level window `pid` owns and
 * prints how many it posted to. A failed PostMessage throws with the Win32 error.
 */
export function closeScript(pid: number): string {
  if (!Number.isSafeInteger(pid) || pid <= 0) {
    throw new Error(`not a PID: ${String(pid)}`);
  }
  return `$ErrorActionPreference = 'Stop'
Add-Type -Namespace ClauseoE2E -Name Win -MemberDefinition @'
[DllImport("user32.dll", CharSet = CharSet.Unicode)]
public static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string cls, string title);
[DllImport("user32.dll")]
public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
[DllImport("user32.dll", SetLastError = true)]
public static extern bool PostMessage(IntPtr hwnd, uint msg, IntPtr w, IntPtr l);
'@
$posted = 0
$hwnd = [IntPtr]::Zero
while ($true) {
  $hwnd = [ClauseoE2E.Win]::FindWindowEx([IntPtr]::Zero, $hwnd, '${WINDOW_CLASS}', [NullString]::Value)
  if ($hwnd -eq [IntPtr]::Zero) { break }
  $owner = [uint32]0
  [void][ClauseoE2E.Win]::GetWindowThreadProcessId($hwnd, [ref]$owner)
  if ($owner -ne ${pid}) { continue }
  if (-not [ClauseoE2E.Win]::PostMessage($hwnd, ${WM_CLOSE}, [IntPtr]::Zero, [IntPtr]::Zero)) {
    throw "PostMessage(WM_CLOSE) to window $hwnd failed: Win32 error $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
  }
  $posted++
}
$posted
`;
}

/** How many windows the script's output says it posted to; throws on anything but a count. */
export function parsePosted(stdout: string): number {
  const match = /^\s*(\d+)\s*$/.exec(stdout);
  if (match?.[1] === undefined) {
    throw new Error(`the close script printed ${JSON.stringify(stdout)}, not a window count`);
  }
  return Number(match[1]);
}

/**
 * Posts WM_CLOSE to the app's main window. Throws, naming why, if PowerShell fails or `pid` owns
 * no `Tauri Window`.
 */
export async function closeWindow(pid: number): Promise<void> {
  const posted = parsePosted(await powershellScript(closeScript(pid)));
  if (posted === 0) {
    throw new Error(`process ${pid} owns no top-level "${WINDOW_CLASS}" window to close`);
  }
}
