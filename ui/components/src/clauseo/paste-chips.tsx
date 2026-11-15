// The long pastes on the composer's well (paste-chips-logic.ts): a chip each, in paste order, beside the papers'
// pills, saying "Pasted text" and its size in words, with a remove button. A chip can't be edited: the lawyer removes
// it and pastes again. The composer holds them through usePasteChips (paste-chips-state.ts).
import { useRef } from "react";
import { IconNewMatter } from "../icons/icons.tsx";
import { count } from "../lib/money.ts";
import { useThreadWords } from "../lib/words-thread.ts";
import type { PasteChip } from "./paste-chips-logic.ts";

/** Each chip: "Pasted text", its words under it, and Remove pasted text, which the size describes. Tab reaches each
 * remove button; a removed chip's focus goes to the next chip's button, else the one before, else the field
 * (`onEmptied`). */
export function PasteChips({
  chips,
  onRemove,
  onEmptied,
}: {
  chips: readonly PasteChip[];
  onRemove: (id: number) => void;
  onEmptied: () => void;
}) {
  const w = useThreadWords();
  const list = useRef<HTMLUListElement>(null);
  const remove = (id: number, at: number) => {
    onRemove(id);
    // after React has drawn the list without it
    queueMicrotask(() => {
      const buttons = list.current?.querySelectorAll<HTMLButtonElement>(".q-paper-remove");
      const to = buttons?.[Math.min(at, buttons.length - 1)];
      if (to === undefined) onEmptied();
      else to.focus();
    });
  };
  return (
    <ul ref={list} className="q-papers q-pastes" data-testid="paste-chips">
      {chips.map((c, at) => (
        <li key={c.id} className="q-paper q-paste" data-testid="paste-chip">
          <span className="q-paste-words">
            <span className="q-paste-name">{w.th("Pasted text")}</span>
            <span className="q-paper-pb" id={`paste-chip-${c.id}`}>
              {c.words === 1 ? w.th("1 word") : w.th("{count} words", { count: count(c.words) })}
            </span>
          </span>
          <button
            type="button"
            className="q-paper-remove"
            aria-label={w.th("Remove pasted text")}
            aria-describedby={`paste-chip-${c.id}`}
            onClick={() => remove(c.id, at)}
          >
            <IconNewMatter size={14} className="rotate-45" />
          </button>
        </li>
      ))}
    </ul>
  );
}
