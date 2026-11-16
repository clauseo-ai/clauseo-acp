// How the composer edits its own field and reads presses on it, apart from the component: every change through the
// engine's own editing, so undo takes it back, and the focus moved only where the lawyer left nothing else.

import { sharedStart } from "./composer-diff.ts";

/** The longest text replaceAll puts in through the engine's editing. Chromium's insertText puts multi-line text in a
 * line at a time, each line an edit and two input events of its own, so its time grows with the square of the text: in
 * a bare textarea 150 KB took 0.7 s, 300 KB 2.7 s and 600 KB 13.6 s, where a paste of 6 MiB takes 0.5 s. Longer text,
 * such as a long question coming back after its turn stopped, is set in place, and that one step is the field's own to
 * undo (placedUndo), since the engine's undo can't take back a value set in place. */
export const UNDOABLE_MAX = 64 * 1024;

/** A text over UNDOABLE_MAX set in place, per field: what the field held before, the text set, and whether it is
 * undone. Cmd/Ctrl+Z puts the words before back while the field still holds that text, as the engine's undo would
 * have (#978), and Cmd/Ctrl+Shift+Z puts the text back while the field still holds the words before. One step, the
 * last; any other replaceAll forgets it. */
const placed = new WeakMap<
  HTMLTextAreaElement,
  { before: string; text: string; undone: boolean }
>();
/** The fields placedUndo listens on. */
const listening = new WeakSet<HTMLTextAreaElement>();

/** Cmd/Ctrl+Z on a field whose last replaceAll set a long text in place, and that still holds it: the words before
 * back; Cmd/Ctrl+Shift+Z after it, or Ctrl+Y on Windows (the page's data-platform, display.ts), the text again. Told to the field as the engine's undo and redo tell it (an input
 * event of historyUndo or historyRedo). Once the field holds anything else, the step is forgotten. */
function placedUndo(e: KeyboardEvent) {
  const a = e.currentTarget;
  if (!(a instanceof HTMLTextAreaElement)) return;
  const step = placed.get(a);
  if (step === undefined || e.isComposing || e.altKey) return;
  // either modifier, never both: on a Mac Ctrl+Z does nothing of its own in a field
  if (e.metaKey === e.ctrlKey) return;
  const key = e.key.toLowerCase();
  // Windows's own redo; on a Mac Cmd+Y is no redo
  const ctrlY =
    key === "y" &&
    e.ctrlKey &&
    !e.shiftKey &&
    a.ownerDocument.documentElement.dataset["platform"] === "windows";
  if (key !== "z" && !ctrlY) return;
  const redo = e.shiftKey || ctrlY;
  if (a.value !== (step.undone ? step.before : step.text)) {
    placed.delete(a);
    return;
  }
  // a redo with nothing undone, or a second undo, is the engine's
  if (redo !== step.undone) return;
  e.preventDefault();
  step.undone = !redo;
  a.value = redo ? step.text : step.before;
  a.dispatchEvent(
    new InputEvent("input", { bubbles: true, inputType: redo ? "historyRedo" : "historyUndo" }),
  );
}

/** Replaces all the field holds with `text` through the engine's editing, so undo takes it back; where the engine
 * refuses (a disabled field, or no execCommand, as in happy-dom), or the text is over UNDOABLE_MAX, the value is set
 * in place, a text over UNDOABLE_MAX with its own undo step (placedUndo). True when the engine did it, in which case
 * its own input event has already told the field. */
export function replaceAll(a: HTMLTextAreaElement, text: string, focus: boolean): boolean {
  placed.delete(a);
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
  const before = a.value;
  a.value = text;
  if (text.length > UNDOABLE_MAX) {
    placed.set(a, { before, text, undone: false });
    if (!listening.has(a)) {
      listening.add(a);
      a.addEventListener("keydown", placedUndo);
    }
  }
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
