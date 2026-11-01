import { useContext } from "react";
import type { ReactNode } from "react";
import { AnswerScopeContext } from "./host.ts";
import { CLOBBER_PREFIX } from "./schema.ts";

/**
 * The answer's notes section, after its last block: a visually hidden heading that each reference names as
 * its description (aria-describedby), and the list of notes, each its own memoised block (`children`).
 * The markup is remark-rehype's footer, with the heading's id scoped to the answer.
 */
export function Notes({ children }: { children: ReactNode }) {
  const scope = useContext(AnswerScopeContext);
  return (
    <section data-footnotes="" className="footnotes">
      <h2 className="sr-only" id={`${CLOBBER_PREFIX}${scope}-footnote-label`}>
        Footnotes
      </h2>
      <ol>{children}</ol>
    </section>
  );
}
