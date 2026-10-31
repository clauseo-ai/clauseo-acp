import { createContext, use } from "react";
import {
  citationsOf,
  citedPaper,
  citeIn,
  paperIn,
  type AnswerId,
  type Cite,
  type CitationId,
  type CitedPaper,
  type MatterData,
  type Paper,
} from "./matter.ts";

const NO_MATTER: MatterData = { papers: [], cites: {}, citations: {}, inView: null };

/** The matter's papers and judgments, and each answer's citations, for the parts that name them by id. The app
 * provides it around a room. */
export const MatterContext = createContext<MatterData>(NO_MATTER);

/** The answer whose words are being rendered, so its references look among its own citations. AnswerBlocks
 * provides it; outside an answer there is none, and a paper's reference resolves to nothing. */
export const AnswerContext = createContext<AnswerId | null>(null);

/** The matter in reach, with lookups by id. Outside a provider it has no papers, no judgments and no citations. */
export function useMatter(): MatterData & {
  paper: (id: string) => Paper | undefined;
  cite: (id: string) => Cite | undefined;
  /** One answer's citation by its id, with the paper (see citedPaper). */
  cited: (answer: AnswerId, id: CitationId) => CitedPaper | undefined;
} {
  const matter = use(MatterContext);
  return {
    ...matter,
    paper: (id) => paperIn(matter, id),
    cite: (id) => citeIn(matter, id),
    cited: (answer, id) => citedPaper(citationsOf(matter, answer), matter.papers, id),
  };
}
