// Quit's key on Windows: Ctrl+Q, in the room and in the settings window, quits the app through `quit_app`, the way the
// Mac's ⌘Q does, and the window's system menu's Quit Clauseo (crates/desktop/src/window_frame/quit_item.rs) does
// too. Only Windows: on the Mac the native menu holds ⌘Q and the page leaves it; Linux keeps the window's close.
// Ctrl+Alt+Q is AltGr+Q, which types "@" on German and other layouts, so any Alt, AltGr, Shift or Meta leaves the key
// to the field. Which key quits is the letter the layout types, not the key's place: on AZERTY the Q key's place is
// labelled A, so Ctrl+A there must not quit, and Ctrl+Q there is the A key. Only when the layout types no Latin
// letter (Russian, Hindi) does the Q key's place decide, so Ctrl+Q still quits there. An input method's composing key,
// a held key's repeats, and a key that types neither a letter nor a mark (Dvorak's ' at the Q key) are not a quit.
import type { Display } from "@clauseo/components/lib/display.ts";
import { useEffect } from "react";
import type { HostTransport } from "../transport/host-transport.ts";
import { quietly } from "./open.ts";

/** Whether `e` is Quit's key on `platform`: plain Ctrl+Q on Windows, never elsewhere. */
export function isQuitKey(e: KeyboardEvent, platform: Display["platform"]): boolean {
  if (platform !== "windows") return false;
  if (e.isComposing || e.keyCode === 229 || e.repeat) return false;
  if (!e.ctrlKey || e.altKey || e.shiftKey || e.metaKey) return false;
  if (e.getModifierState("AltGraph")) return false;
  return quitsByLetter(e);
}

/** The letter Ctrl types decides: a Latin letter quits only when it is q; another letter or mark quits at the Q key. */
function quitsByLetter(e: KeyboardEvent): boolean {
  if (/^[a-z]$/i.test(e.key)) return e.key.toLowerCase() === "q";
  if (/^[\p{L}\p{M}]$/u.test(e.key)) return e.code === "KeyQ";
  return false;
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
