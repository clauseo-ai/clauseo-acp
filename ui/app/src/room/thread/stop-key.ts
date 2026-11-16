// Stop's keys (D-058): while an answer runs in the open conversation, Escape, and ⌘. on the Mac or Ctrl+. on Windows
// and Linux, stop it as the composer's Stop does, through the same call. Not while the focus is in a menu, a dialog,
// a popover or a list box, or one of those stands open, which hears Escape itself; Escape not while a prompt in the
// thread holds it (a permission's Don't allow, a question's Don't answer: their aria-keyshortcuts say so); not in the floating rail, which
// folds on it, or in Find; not for a key another handler already took (the composer's `/` and `@` menus, Find's clear); and not
// for an input method's composing key.
import { useEffect } from "react";

/** Where Escape belongs to something else: a menu, a dialog, a popover (Base UI's are dialogs), a list box, and the
 * rail while it floats open over the work, and Find, whose Escape clears its words or brings the date groups back. */
const OWNS_ESCAPE =
  '[role="menu"], [role="menubar"], [role="dialog"], [role="alertdialog"], [role="listbox"], .room-rail[data-open], .room-find';

/** A menu, dialog or popover open anywhere on the page: Base UI's popups carry `data-open` while they stand. */
const OPEN_POPUP =
  '[role="menu"][data-open], [role="dialog"][data-open], [role="alertdialog"], [aria-modal="true"]';

/** Whether `e` is one of Stop's keys: Escape alone, or ⌘. (Mac) or Ctrl+. (elsewhere) with no other modifier. */
export function isStopKey(e: KeyboardEvent, mac: boolean): boolean {
  if (e.isComposing || e.keyCode === 229 || e.altKey || e.shiftKey) return false;
  if (e.key === "Escape") return !e.metaKey && !e.ctrlKey;
  if (e.key !== "." && e.code !== "Period") return false;
  return mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
}

/** Whether a control on the page holds Escape now: a permission prompt's Don't allow, a question's Don't answer, the
 * Safe workspace's prompt (each names it in aria-keyshortcuts while its keys are live), unless disabled or inert. */
function escapeHeld(): boolean {
  return [...document.querySelectorAll<HTMLElement>('[aria-keyshortcuts~="Escape"]')].some(
    (el) => !el.matches(":disabled") && el.closest("[inert]") === null,
  );
}

/** Whether Stop's key `e` is the page's to stop with: taken by no one, and not where Escape is someone else's. */
function free(e: KeyboardEvent): boolean {
  if (e.defaultPrevented) return false;
  const target = e.target instanceof Element ? e.target : null;
  if (target?.closest(OWNS_ESCAPE) != null) return false;
  if (document.querySelector(OPEN_POPUP) !== null) return false;
  return e.key !== "Escape" || !escapeHeld();
}

/** Calls `stop` on Stop's key while `busy`: the composer's own Stop, so a key and a press do the same. */
export function useStopKey(busy: boolean, stop: () => void, mac: boolean): void {
  useEffect(() => {
    if (!busy) return;
    const onKey = (e: KeyboardEvent) => {
      if (!isStopKey(e, mac) || !free(e)) return;
      e.preventDefault();
      stop();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [busy, stop, mac]);
}
