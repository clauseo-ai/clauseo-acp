// The composer's long pastes as they meet its field (composer.tsx): a paste of PASTE_CHIP_MIN units or more made a
// chip, over selected words too; Cmd/Ctrl+Z taking a chip step back; and the chips a Draft sets, keeping those already
// in their place. The chips' list and steps are usePasteChips's (paste-chips-state.ts).
import type { ClipboardEvent, KeyboardEvent } from "react";
import {
  chipOf,
  draftOf,
  PASTE_CHIP_MIN,
  type Draft,
  type PasteChip,
} from "./paste-chips-logic.ts";
import type { Step, usePasteChips } from "./paste-chips-state.ts";

type Chips = ReturnType<typeof usePasteChips>;

/** What the composer is given, as a Draft: a string is typed words for the field, or one chip, whole, when it is
 * PASTE_CHIP_MIN units or more (draftOf), so the field never takes a long text. */
export const draftIn = (text: string | Draft): Draft =>
  typeof text === "string" ? draftOf(text) : text;

/** The chips for `pastes`, given the chips `now`: a chip kept in its place is kept, with its words' count; the same
 * list when nothing changed. */
export function chipsFor(
  now: readonly PasteChip[],
  pastes: readonly string[],
): readonly PasteChip[] {
  const same = pastes.length === now.length && pastes.every((t, i) => t === now[i]!.text);
  return same ? now : pastes.map((t, i) => (now[i]?.text === t ? now[i]! : chipOf(t)));
}

/** A paste on the field, outside a composition: one of PASTE_CHIP_MIN units or more is a chip, not words in the field
 * (paste-chips-logic.ts), one step undo takes back. Over selected words it takes them out, as any paste does. */
export function pasteLong(
  e: ClipboardEvent<HTMLTextAreaElement>,
  chips: Chips,
  changed: () => void,
): void {
  const text = e.clipboardData.getData("text/plain");
  if (text.length < PASTE_CHIP_MIN) return;
  e.preventDefault();
  const a = e.currentTarget;
  const was = a.value;
  const [from, to] = [a.selectionStart, a.selectionEnd];
  // the chip first, so the input event of the selection's removal reads it
  const step = chips.add(text, was);
  if (from === to) {
    changed();
    return;
  }
  // through the engine's editing where it can, so its undo puts the words back with the chip's; else in place, and
  // the step puts them back
  const doc = a.ownerDocument;
  const engine =
    typeof doc.execCommand === "function" && doc.execCommand("delete") && a.value !== was;
  if (!engine) {
    a.value = was.slice(0, from) + was.slice(to);
    a.setSelectionRange(from, from);
    step.was = was;
  }
  step.box = a.value;
  step.engine = engine;
  if (!engine) changed();
}

/** A Cmd/Ctrl+Z the chips took (usePasteChips's undo): with its words not the engine's to undo, nothing more of the
 * key, the field's words put back if they were taken out in place, and the room told. True when the key is done. */
export function undone(
  e: KeyboardEvent<HTMLElement>,
  step: Step | null,
  a: HTMLTextAreaElement,
  changed: () => void,
): boolean {
  if (step === null || step.engine) return false;
  e.preventDefault();
  if (step.was !== undefined) a.value = step.was;
  changed();
  return true;
}
