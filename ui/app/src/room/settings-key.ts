// Settings' key in the room: Ctrl+, on Windows and Linux opens the settings window, as the rail's Settings does. On the
// Mac the native menu's Settings item holds ⌘,; the room hears ⌘, too, in case the key reaches the page, and the host's
// open-or-focus makes a second open only bring the one window forward. An input method's composing key is its own.
import { useEffect } from "react";

/** Whether `e` is Settings' key: ⌘, on the Mac, Ctrl+, elsewhere, with no other modifier, and not composing. The comma
 * the layout gives, or the comma key's place where the layout gives no Latin character there. */
export function isSettingsKey(e: KeyboardEvent, mac: boolean): boolean {
  if (e.isComposing || e.keyCode === 229 || e.altKey || e.shiftKey) return false;
  if (mac ? !e.metaKey || e.ctrlKey : !e.ctrlKey || e.metaKey) return false;
  return e.key === "," || (e.code === "Comma" && !/^[\x20-\x7e]$/.test(e.key));
}

/** Calls `open` on Settings' key while the room is mounted; nothing without `open` (no settings window to open). */
export function useSettingsKey(open: (() => void) | undefined, mac: boolean): void {
  useEffect(() => {
    if (open === undefined) return;
    const onKey = (e: KeyboardEvent) => {
      if (!isSettingsKey(e, mac)) return;
      e.preventDefault();
      open();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, mac]);
}
