// The composer's hold on its long pastes' chips (paste-chips.tsx draws them): the list, the chips' texts, and their
// part in the field's undo.
import { useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { chipOf, type PasteChip } from "./paste-chips-logic.ts";

/** A change of the chips, alone (a paste, a chip's remove button) or with a change of the field's words (a paste
 * over selected words, a clear, fill or set through the handle), which Cmd/Ctrl+Z takes back, with the field's: `box`
 * is what the field held after it, `engine` whether the engine's undo holds the field's part of it, and `was`, when
 * the field's part was made in place, what the field held before it. */
export type Step = {
  before: readonly PasteChip[];
  after: readonly PasteChip[];
  box: string;
  engine: boolean;
  was?: string;
};

/** The composer's chips: the list to draw, read through a ref so a keystroke never waits on a render, and the chips'
 * texts listed once per change, for the composer's Draft (paste-chips-logic.ts). */
export function usePasteChips(initial: readonly PasteChip[]) {
  const [list, setList] = useState(initial);
  const now = useRef(initial);
  const texts = useRef(initial.map((c) => c.text));
  const steps = useRef<Step[]>([]);
  const put = (next: readonly PasteChip[]) => {
    if (next === now.current) return;
    now.current = next;
    texts.current = next.map((c) => c.text);
    setList(next);
  };
  // a step recorded and made; at most 16 kept
  const push = (step: Step): Step => {
    steps.current.push(step);
    if (steps.current.length > 16) steps.current.shift();
    put(step.after);
    return step;
  };
  return {
    list,
    /** The chips' texts, in paste order: the same array until the chips change. */
    pastes: () => texts.current,
    /** The chips now, read without a render. */
    now: () => now.current,
    has: () => now.current.length > 0,
    /** A paste of `text`, as the last chip: one step Cmd/Ctrl+Z takes back, the field holding `box`. The step comes
     * back for a paste over selected words to say how they went. */
    add: (text: string, box: string): Step =>
      push({ before: now.current, after: [...now.current, chipOf(text)], box, engine: false }),
    /** A chip's remove button: one step Cmd/Ctrl+Z takes back, from the field or from a chip's button, so a stray
     * press never loses the paste. `box` is what the field holds. */
    remove: (id: number, box: string) => {
      push({
        before: now.current,
        after: now.current.filter((c) => c.id !== id),
        box,
        engine: false,
      });
    },
    /** The chips set with the field's words (handle's setText, clear or fill), before the field's change, so the
     * input event it fires reads them; `box` as Step says. The step comes back for its `engine`, known once the
     * field is changed. */
    set: (next: readonly PasteChip[], box: string): Step | null => {
      if (next.length === 0 && now.current.length === 0) return null;
      return push({ before: now.current, after: next, box, engine: false });
    },
    /** Drops every step: after an accepted send, so Cmd/Ctrl+Z never brings the sent question's chips back. */
    forget: () => {
      steps.current = [];
    },
    /** Cmd/Ctrl+Z, on the field (`inField`) or on a chip's button, the field holding `box`: when the next undo is
     * of a step that changed the chips, their change is taken back too, and the step comes back. When its `engine` is
     * false the key must do nothing more (and the field's words go back to its `was`, if it has one); when true, the
     * engine's own undo takes the field's part back. Off the field, a step whose words the engine holds is left for
     * the field's own undo. */
    undo: (e: ReactKeyboardEvent, mac: boolean, box: string, inField: boolean): Step | null => {
      const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
      if (!mod || e.shiftKey || e.altKey || e.key.toLowerCase() !== "z") return null;
      const top = steps.current.at(-1);
      if (top === undefined || top.after !== now.current || top.box !== box) return null;
      if (top.engine && !inField) return null;
      steps.current.pop();
      put(top.before);
      return top;
    },
  };
}
