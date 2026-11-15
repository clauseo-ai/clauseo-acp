// Quit's key on Windows: Ctrl+Q, in the room and in the settings window, quits the app through `quit_app`, the way the
// Mac's ⌘Q does, and the window's system menu's Quit Clauseo (crates/desktop/src/window_frame/quit_item.rs) does
// too. Only Windows: on the Mac the native menu holds ⌘Q and the page leaves it; Linux keeps the window's close.
// Ctrl+Alt+Q is AltGr+Q, which types "@" on German and other layouts, so any Alt, AltGr, Shift or Meta leaves the key
// to the field. The Q key's place, so a layout with no Latin Q there still quits; an input method's composing key, or
// a held key's repeats, are not a quit.
import type { Display } from "@clauseo/components/lib/display.ts";
import { useEffect } from "react";
import type { HostTransport } from "../transport/host-transport.ts";
import { quietly } from "./open.ts";

/** Whether `e` is Quit's key on `platform`: plain Ctrl+Q on Windows, never elsewhere. */
export function isQuitKey(e: KeyboardEvent, platform: Display["platform"]): boolean {
  if (platform !== "windows" || e.code !== "KeyQ") return false;
  if (e.isComposing || e.keyCode === 229 || e.repeat) return false;
  if (!e.ctrlKey || e.altKey || e.shiftKey || e.metaKey) return false;
  return !e.getModifierState("AltGraph");
}

/** Quits through `transport` on Quit's key while the page is mounted. A refusal goes to the console (open.ts). */
export function useQuitKey(transport: HostTransport, platform: Display["platform"]): void {
  useEffect(() => {
    if (platform !== "windows") return;
    const onKey = (e: KeyboardEvent) => {
      if (!isQuitKey(e, platform)) return;
      e.preventDefault();
      void quietly("quit_app", transport.quitApp());
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [transport, platform]);
}
