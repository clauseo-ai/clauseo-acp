// The composer's handle (composer.tsx): what the room may do to its field from outside.
import type { MentionItem } from "./composer.tsx";
import type { Draft } from "./paste-chips-logic.ts";

/** What the room may do to the field from outside. The field is uncontrolled: React never writes its value, so the
 * engine's undo stack stays whole through an input method's edits, and every change made here goes through the
 * engine's own editing (`execCommand`), so Cmd/Ctrl+Z takes it back. */
export type ComposerHandle = {
  focus: () => void;
  /** What goes if sent now, as one string: each long paste's text, then the typed words (paste-chips-logic.ts's
   * sentOf). Made on each call: for a send, or to check what was sent, never per keystroke. */
  text: () => string;
  /** What the composer holds now, its typed words and its long pastes' texts apart (paste-chips-logic.ts's Draft). */
  draft: () => Draft;
  /** Puts the field's focus in it and replaces all it holds with `text`, as one step undo takes back. A string is
   * typed words (one chip when it is PASTE_CHIP_MIN units or more); a Draft brings its chips with it. */
  setText: (text: string | Draft) => void;
  /** Empties the field as one undoable step: with the field focused, or with focus on nothing or on a button of the
   * field's own row (Ask, just pressed), which it takes back to the field. With focus anywhere else on the page it is
   * emptied in place, so the lawyer's focus never moves, and that one step can't be undone. Given `only`, the
   * question just sent and accepted, it clears only while the composer holds exactly that, and Cmd/Ctrl+Z after it
   * brings back no chip: the long pastes went with the question. While an input method composes it waits for the
   * composition's end and clears then if the field still holds what it held. */
  clear: (only?: string) => void;
  /** Puts `text` into the field only while it is empty, as one undoable step, and never moves the lawyer's focus:
   * it takes the focus only where clear may (mayTakeFocus), and anywhere else the words go in place. While an input
   * method composes it waits for the composition's end, and fills then only if the field is still empty. */
  fill: (text: string | Draft) => void;
  /** Adds `text` after what the field holds, a blank line between (into an empty field as fill does), and never moves
   * the caret or the focus: while the lawyer is in the field, at its end through the engine, as one undoable step;
   * elsewhere in place. While an input method composes it waits for the composition's end. */
  append: (text: string | Draft) => void;
  /** The things the field's `@` tokens mention now, in the order they first stand in it, each once: what a send of
   * its words names alongside them. A token edited or deleted mentions nothing. */
  mentioned: () => readonly MentionItem[];
};
