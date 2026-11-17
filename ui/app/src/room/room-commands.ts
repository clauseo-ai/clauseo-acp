// The room's commands from outside the page and their keys: New conversation (⌘N on the Mac, Ctrl+N elsewhere, while
// the open matter's conversations are in), New matter (⇧⌘N, Ctrl+Shift+N) and Find (⌘K, find-key.ts). The Mac's
// native menu (crates/desktop/src/menu.rs) sends each as a `clauseo-menu` event on the window, its detail the
// command's name, so a menu item and its key do the same; a key the page takes never reaches the menu, and a key the
// page leaves (none to do) reaches the menu, which sends the event the page then leaves too. Each key is the letter
// the layout gives, or its key's place where the layout gives no Latin letter (Devanagari, Cyrillic); an input
// method's composing key is its own. The sidebar and the papers fold on the View menu's two items on the Mac, ⌃⌘S
// and ⌥⌘I, whose keys the page leaves to the menu so one press folds once; on Windows and Linux, with no menu, on the
// page's own Ctrl+B and Ctrl+Alt+B (isLayoutKey).
import { useEffect } from "react";

/** The event the native menu sends the page. */
export const MENU_EVENT = "clauseo-menu";

/** A command the native menu sends. */
export type MenuCommand =
  | "new_conversation"
  | "new_matter"
  | "find"
  | "toggle_sidebar"
  | "toggle_papers";

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

/** Whether `e` is the sidebar's key (Ctrl+B) or, with `alt`, the papers' (Ctrl+Alt+B), on Windows and Linux; never on
 * the Mac, where the View menu's items hold their keys. The B key's place, whatever letter the layout gives it, and
 * no Shift or Windows key. AltGr, which Windows reports as Ctrl+Alt, types characters on Indian and European layouts,
 * so a key with AltGraph down is never the papers'. */
export function isLayoutKey(e: KeyboardEvent, alt: boolean, mac: boolean): boolean {
  if (mac || e.isComposing || e.keyCode === 229) return false;
  if (!e.ctrlKey || e.metaKey || e.shiftKey || e.altKey !== alt) return false;
  if (alt && e.getModifierState("AltGraph")) return false;
  return e.code === "KeyB";
}

/** The room's commands: each null while it can't run (no conversations in, a dialog holding the keys). */
export type RoomCommands = {
  newConversation: (() => void) | null;
  newMatter: (() => void) | null;
  find: () => void;
  /** Shows or hides the sidebar, as its toolbar button does. */
  toggleSidebar: (() => void) | null;
  /** Shows or hides the papers, as their toolbar button does. */
  togglePapers: (() => void) | null;
};

/** New conversation's and New matter's keys, the sidebar's and the papers' off the Mac, and the native menu's five
 * commands, while the room is mounted. */
export function useRoomCommands(commands: RoomCommands, mac: boolean): void {
  const { newConversation, newMatter, find, toggleSidebar, togglePapers } = commands;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const run = isCommandKey(e, "n", false, mac)
        ? newConversation
        : isCommandKey(e, "n", true, mac)
          ? newMatter
          : isLayoutKey(e, false, mac)
            ? toggleSidebar
            : isLayoutKey(e, true, mac)
              ? togglePapers
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
      else if (command === "toggle_sidebar") toggleSidebar?.();
      else if (command === "toggle_papers") togglePapers?.();
    };
    document.addEventListener("keydown", onKey);
    window.addEventListener(MENU_EVENT, onMenu);
    return () => {
      document.removeEventListener("keydown", onKey);
      window.removeEventListener(MENU_EVENT, onMenu);
    };
  }, [newConversation, newMatter, find, toggleSidebar, togglePapers, mac]);
}
