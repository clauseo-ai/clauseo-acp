// Find's key in the room (U16a): ⌘K on the Mac, Ctrl+K on Windows and Linux, puts the focus in the rail's Find from
// anywhere in the room, opening a folded rail first. As New conversation's key (room.tsx), the letter the layout gives,
// or the K key's place where the layout gives no Latin letter; no other modifier, and an input method's composing key
// is its own.
import { useEffect } from "react";
import { flushSync } from "react-dom";

/** Whether `e` is Find's key: ⌘K on the Mac, Ctrl+K elsewhere. */
export function isFindKey(e: KeyboardEvent, mac: boolean): boolean {
  if (e.isComposing || e.keyCode === 229 || e.altKey || e.shiftKey) return false;
  if (mac ? !e.metaKey || e.ctrlKey : !e.ctrlKey || e.metaKey) return false;
  const key = e.key.toLowerCase();
  return /^[a-z]$/.test(key) ? key === "k" : e.code === "KeyK";
}

/** Focuses the field `field` names; where it can't take focus (the rail folded away), `openRail` opens the rail
 * first. False where the room shows no Find (no matters, or Versions in the rail's place). The native menu's Find
 * (room-commands.ts) comes here too. */
export function focusFind(field: () => HTMLElement | null, openRail: () => void): boolean {
  const input = field();
  if (input === null) return false;
  input.focus();
  if (document.activeElement === input) return true;
  flushSync(openRail);
  field()?.focus();
  return true;
}

/** On Find's key, focuses Find (focusFind). Nothing where the room shows no Find. */
export function useFindKey(
  field: () => HTMLElement | null,
  openRail: () => void,
  mac: boolean,
): void {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!isFindKey(e, mac) || field() === null) return;
      e.preventDefault();
      focusFind(field, openRail);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [field, openRail, mac]);
}
