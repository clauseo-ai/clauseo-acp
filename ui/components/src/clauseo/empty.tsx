// The empty states. Each says what goes here and what Clauseo will do with it, with one way
// forward where there is one: a matter with no papers yet, a new matter's first question, and a search that finds
// nothing.
import { Button } from "../ui/button.tsx";
import { IconAddPapers, IconSearch } from "../icons/icons.tsx";
import { useThreadWords } from "../lib/words-thread.ts";
import { Say } from "./say.tsx";

/** The wall of a matter with no papers yet, in the facade's place: what goes here, that Clauseo reads it before the
 * first question, and a tonal Add papers. */
export function EmptyPapers({ onAdd }: { onAdd?: () => void }) {
  const w = useThreadWords();
  return (
    <div className="empty-papers">
      <p className="empty-title">
        <Say>{w.t("No papers yet.")}</Say>
      </p>
      <p className="empty-body">
        <Say>
          {w.t("Add the matter's papers and Clauseo reads them before your first question.")}
        </Say>
      </p>
      <Button kind="tone" className="empty-action" onClick={onAdd}>
        <IconAddPapers size={16} />
        <span className="deva-nudge">{w.t("Add papers")}</span>
      </Button>
    </div>
  );
}

/** A new matter's thread before its first question: that the papers are read, and where the question goes. It sits
 * at the thread's foot, over the composer, on the reading column. */
export function FirstQuestion({ papers }: { papers: number }) {
  const w = useThreadWords();
  return (
    <div className="first-q">
      <p className="first-q-title">
        <Say>{w.th("Clauseo has read this matter's {n} papers.", { n: papers })}</Say>
      </p>
      <p className="first-q-body">
        <Say>
          {w.th("Ask the first question below. Each answer cites the paper and page it relies on.")}
        </Say>
      </p>
    </div>
  );
}

/** What search says when nothing matches: the words searched for, as typed, and what search covers. */
export function SearchEmpty({ query }: { query: string }) {
  const w = useThreadWords();
  return (
    <div className="search-empty" role="status">
      <IconSearch size={16} className="search-empty-icon" />
      <div>
        <p className="search-empty-title">
          <Say>{w.th("Nothing in this matter matches '{q}'.", { q: query })}</Say>
        </p>
        <p className="search-empty-body">
          <Say>{w.th("Search covers the papers, the answers and Versions.")}</Say>
        </p>
      </div>
    </div>
  );
}
