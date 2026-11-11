// How the composer edits its own field and reads presses on it, apart from the component: every change through the
// engine's own editing, so undo takes it back, and the focus moved only where the lawyer left nothing else.

import { sharedStart } from "./composer-diff.ts";

/** The longest text replaceAll puts in through the engine's editing. Chromium's insertText puts multi-line text in a
 * line at a time, each line an edit and two input events of its own, so its time grows with the square of the text: in
 * a bare textarea 150 KB took 0.7 s, 300 KB 2.7 s and 600 KB 13.6 s, where a paste of 6 MiB takes 0.5 s. Longer text,
 * such as a long question coming back after its turn stopped, is set in place, and that one step can't be undone. */
export const UNDOABLE_MAX = 64 * 1024;

/** Replaces all the field holds with `text` through the engine's editing, so undo takes it back; where the engine
 * refuses (a disabled field, or no execCommand, as in happy-dom), or the text is over UNDOABLE_MAX, the value is set
 * in place. True when the engine did it, in which case its own input event has already told the field. */
export function replaceAll(a: HTMLTextAreaElement, text: string, focus: boolean): boolean {
  if (a.value === text) return true;
  const focused = a.ownerDocument.activeElement === a;
  if (
    !a.disabled &&
    (focused || focus) &&
    text.length <= UNDOABLE_MAX &&
    typeof a.ownerDocument.execCommand === "function"
  ) {
    if (!focused) a.focus();
    a.select();
    // insertText and delete edit plain text only: nothing here parses HTML, so Trusted Types have nothing to check
    const doc = a.ownerDocument;
    const ok = text === "" ? doc.execCommand("delete") : doc.execCommand("insertText", false, text);
    if (ok && a.value === text) return true;
  }
  a.value = text;
  return false;
}

/** Whether clearing may move the focus to the field: focus is on nothing, on the field itself, or on a button of the
 * field's own row, as after a press on Ask. Anywhere else the lawyer has gone on to something and keeps it. */
export function mayTakeFocus(a: HTMLTextAreaElement): boolean {
  const doc = a.ownerDocument;
  const now = doc.activeElement;
  if (now === null || now === doc.body || now === a) return true;
  const row = a.closest(".room-field");
  return now instanceof HTMLButtonElement && row !== null && now.closest(".room-field") === row;
}

/** Whether a press in the field landed on a button or the text itself, which take it themselves. */
export function pressOwn(target: EventTarget, field: Element): boolean {
  for (
    let n = target instanceof Element ? target : null;
    n !== null && n !== field;
    n = n.parentElement
  ) {
    if (n instanceof HTMLButtonElement || n instanceof HTMLTextAreaElement) return true;
  }
  return false;
}

/** Whether `next` is `was` with words put in at one place and nothing taken out, as plain typing and a paste do, or
 * the same words. */
export function insertedOnly(was: string, next: string): boolean {
  if (next.length < was.length) return false;
  const i = sharedStart(was, next, was.length);
  return next.endsWith(was.slice(i));
}
