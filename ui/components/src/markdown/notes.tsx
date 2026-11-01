import { useContext } from "react";
import type { ReactNode } from "react";
import { useWords } from "../lib/words.ts";
import { AnswerScopeContext } from "./host.ts";
import { CLOBBER_PREFIX } from "./schema.ts";

/**
 * The answer's notes section, after its last block: a visually hidden heading that each reference names as
 * its description (aria-describedby), and the list of notes, each its own memoised block (`children`).
 * The markup is remark-rehype's footer, with the heading's id scoped to the answer.
 */
export function Notes({ children }: { children: ReactNode }) {
  const scope = useContext(AnswerScopeContext);
  const words = useWords();
  return (
    <section data-footnotes="" className="footnotes">
      <h2 className="sr-only" id={`${CLOBBER_PREFIX}${scope}-footnote-label`}>
        {words.t("Footnotes")}
      </h2>
      <ol>{children}</ol>
    </section>
  );
}

type SuperscriptProps = { children?: ReactNode; "data-footnote-pending"?: string | undefined };

/**
 * Every superscript in an answer. A footnote reference whose note has not arrived yet (its number in
 * `data-footnote-pending`) shows the number and says what it is to a screen reader, since it is no link.
 */
export function Superscript({ children, "data-footnote-pending": pending }: SuperscriptProps) {
  const words = useWords();
  if (pending === undefined) return <sup>{children}</sup>;
  return (
    <sup data-footnote-pending="">
      <span aria-hidden="true">{children}</span>
      <span className="sr-only">{words.t("Footnote {n}, still to come", { n: pending })}</span>
    </sup>
  );
}
