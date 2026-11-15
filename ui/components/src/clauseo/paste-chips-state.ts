// The composer's hold on its long pastes' chips (paste-chips.tsx draws them): the list, the chips' joined text, and
// their part in the field's undo.
import { useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { chipOf, chipTail, type PasteChip } from "./paste-chips-logic.ts";

/** A change of the chips made with a change of the field's words (a clear, fill or set through the handle), which
 * Cmd/Ctrl+Z takes back with the field's: `box` is what the field held after it, `engine` whether the engine's undo
 * holds the field's part of it. */
type Step = {
  before: readonly PasteChip[];
  after: readonly PasteChip[];
  box: string;
  engine: boolean;
};

/** The composer's chips: the list to draw, read through a ref so a keystroke never waits on a render, and the chips'
 * text joined once per change (chipTail) for the composer's one string. */
export function usePasteChips(initial: readonly PasteChip[]) {
  const [list, setList] = useState(initial);
  const now = useRef(initial);
  const tail = useRef(chipTail(initial));
  const steps = useRef<Step[]>([]);
  const put = (next: readonly PasteChip[]) => {
    if (next === now.current) return;
    now.current = next;
    tail.current = chipTail(next);
    setList(next);
  };
  return {
    list,
    /** The chips' texts joined, as they follow the typed words. */
    tail: () => tail.current,
    has: () => now.current.length > 0,
    /** A paste of `text`, as the last chip: the lawyer's own change, so no undo step of the field's comes with it. */
    add: (text: string) => {
      steps.current = [];
      put([...now.current, chipOf(text)]);
    },
    remove: (id: number) => {
      steps.current = [];
      put(now.current.filter((c) => c.id !== id));
    },
    /** The chips set with the field's words (handle's setText, clear or fill), before the field's change, so the
     * input event it fires reads them; `box` as Step says. The step comes back for its `engine`, known once the
     * field is changed. */
    set: (next: readonly PasteChip[], box: string): Step | null => {
      if (next.length === 0 && now.current.length === 0) return null;
      const step = { before: now.current, after: next, box, engine: false };
      steps.current.push(step);
      if (steps.current.length > 16) steps.current.shift();
      put(next);
      return step;
    },
    /** Cmd/Ctrl+Z on the field: when the next undo is of a step that changed the chips, their change is taken back
     * too. True when the field's part was not the engine's to undo, so the key must do nothing more. */
    undo: (e: ReactKeyboardEvent<HTMLTextAreaElement>, mac: boolean): boolean => {
      const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
      if (!mod || e.shiftKey || e.altKey || e.key.toLowerCase() !== "z") return false;
      const top = steps.current.at(-1);
      if (top === undefined || top.after !== now.current || top.box !== e.currentTarget.value)
        return false;
      steps.current.pop();
      put(top.before);
      return !top.engine;
    },
  };
}
