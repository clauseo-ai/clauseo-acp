// The room's commands from outside the page and their keys: New conversation (⌘N on the Mac, Ctrl+N elsewhere, while
// the open matter's conversations are in), New matter (⇧⌘N, Ctrl+Shift+N) and Find (⌘K, find-key.ts). The Mac's
// native menu (crates/desktop/src/menu.rs) sends each as a `clauseo-menu` event on the window, its detail the
// command's name, so a menu item and its key do the same; a key the page takes never reaches the menu, and a key the
// page leaves (none to do) reaches the menu, which sends the event the page then leaves too. Each key is the letter
// the layout gives, or its key's place where the layout gives no Latin letter (Devanagari, Cyrillic); an input
// method's composing key is its own.
import { useEffect } from "react";

/** The event the native menu sends the page. */
export const MENU_EVENT = "clauseo-menu";

/** A command the native menu sends. */
export type MenuCommand = "new_conversation" | "new_matter" | "find";

/** Whether `e` is the letter `letter` with the platform's command key (⌘ on the Mac, Ctrl elsewhere), with Shift when
 * `shift`, and no other modifier. */
export function isCommandKey(
  e: KeyboardEvent,
  letter: string,
  shift: boolean,
  mac: boolean,
): boolean {
  if (e.isComposing || e.keyCode === 229 || e.altKey || e.shiftKey !== shift) return false;
  if (mac ? !e.metaKey || e.ctrlKey : !e.ctrlKey || e.metaKey) return false;
  const key = e.key.toLowerCase();
  return /^[a-z]$/.test(key) ? key === letter : e.code === `Key${letter.toUpperCase()}`;
}

/** The room's commands: each null while it can't run (no conversations in, a dialog holding the keys). */
export type RoomCommands = {
  newConversation: (() => void) | null;
  newMatter: (() => void) | null;
  find: () => void;
};

/** New conversation's and New matter's keys, and the native menu's three commands, while the room is mounted. */
export function useRoomCommands(commands: RoomCommands, mac: boolean): void {
  const { newConversation, newMatter, find } = commands;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const run = isCommandKey(e, "n", false, mac)
        ? newConversation
        : isCommandKey(e, "n", true, mac)
          ? newMatter
          : null;
      if (run === null) return;
      e.preventDefault();
      run();
    };
    const onMenu = (e: Event) => {
      const command = e instanceof CustomEvent ? (e.detail as unknown) : null;
      if (command === "new_conversation") newConversation?.();
      else if (command === "new_matter") newMatter?.();
      else if (command === "find") find();
    };
    document.addEventListener("keydown", onKey);
    window.addEventListener(MENU_EVENT, onMenu);
    return () => {
      document.removeEventListener("keydown", onKey);
      window.removeEventListener(MENU_EVENT, onMenu);
    };
  }, [newConversation, newMatter, find, mac]);
}
