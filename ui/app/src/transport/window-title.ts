// The current window's native title through Tauri: what Windows shows in the taskbar and Alt-Tab, and macOS in the
// Window menu. The settings window names itself in the page's language with it. Outside the desktop app it does
// nothing. Best effort: a refusal is logged, never shown.
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

/** Sets the current window's native title; a no-op outside Tauri, a failure only logged. */
export function setWindowTitle(title: string): void {
  if (!isTauri()) return;
  new Promise<void>((resolve) => resolve(getCurrentWindow().setTitle(title))).catch(
    (error: unknown) => console.warn(`window setTitle failed: ${String(error)}`),
  );
}
