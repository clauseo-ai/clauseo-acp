// The current window's label through Tauri ("main" or "settings"), so what the page keeps for its window is kept per
// window. Outside the desktop app (the probe, the tests, a browser) it is "main", the room's own window.
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

/** The current window's label; "main" outside Tauri, or where Tauri can't say. */
export function windowLabel(): string {
  if (!isTauri()) return "main";
  try {
    return getCurrentWindow().label;
  } catch {
    return "main";
  }
}
