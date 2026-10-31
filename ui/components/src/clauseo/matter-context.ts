import { createContext, use } from "react";
import {
  citedPaper,
  citeIn,
  paperIn,
  type Cite,
  type CitedPaper,
  type MatterData,
  type Paper,
} from "./matter.ts";

const NO_MATTER: MatterData = { papers: [], cites: {}, citations: [] };

/** The matter's papers and judgments, and the answer's citations, for the parts that name them by id. The app
 * provides it around a room. */
export const MatterContext = createContext<MatterData>(NO_MATTER);

/** The matter in reach, with lookups by id. Outside a provider it has no papers, no judgments and no citations. */
export function useMatter(): MatterData & {
  paper: (id: string) => Paper | undefined;
  cite: (id: string) => Cite | undefined;
  /** The answer's citation of a paper, with the paper (see citedPaper). */
  cited: (id: string) => CitedPaper | undefined;
} {
  const matter = use(MatterContext);
  return {
    ...matter,
    paper: (id) => paperIn(matter, id),
    cite: (id) => citeIn(matter, id),
    cited: (id) => citedPaper(matter.citations, matter.papers, id),
  };
}
