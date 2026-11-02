// The papers added to a question, as pills on the composer's well.
import { IconNewMatter, IconPaperBookPage } from "../icons/icons.tsx";
import { useThreadWords } from "../lib/words-thread.ts";
import type { QuestionPaper } from "./composer.tsx";
import { pbWords } from "./matter.ts";
import { Say } from "./say.tsx";

/** Each paper's pill: the paper book page glyph, its short title and page, and a press to take it off again. */
export function PaperChips({
  papers,
  onRemove,
}: {
  papers: readonly QuestionPaper[];
  onRemove?: ((id: string) => void) | undefined;
}) {
  const w = useThreadWords();
  return (
    <ul className="q-papers" aria-label={w.th("Papers for this question")}>
      {papers.map((p) => (
        <li key={p.id} className="q-paper">
          <IconPaperBookPage size={14} className="q-paper-icon" />
          <span className="q-paper-title">
            <Say>{p.title}</Say>
          </span>
          <span className="q-paper-pb">{pbWords(w.lang, p.pb)}</span>
          <button
            type="button"
            className="q-paper-remove"
            aria-label={w.th("Remove {title}", { title: p.title })}
            onClick={() => onRemove?.(p.id)}
          >
            <IconNewMatter size={14} className="rotate-45" />
          </button>
        </li>
      ))}
    </ul>
  );
}
