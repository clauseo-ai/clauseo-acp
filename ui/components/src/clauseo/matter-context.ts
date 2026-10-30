import { createContext, use } from "react";
import { citeIn, paperIn, type Cite, type MatterData, type Paper } from "./matter.ts";

const NO_MATTER: MatterData = { papers: [], cites: {} };

/** The matter's papers and judgments, for the parts that name them by id. The app provides it around a room. */
export const MatterContext = createContext<MatterData>(NO_MATTER);

/** The matter in reach, with lookups by id. Outside a provider it has no papers and no judgments. */
export function useMatter(): MatterData & {
  paper: (id: string) => Paper | undefined;
  cite: (id: string) => Cite | undefined;
} {
  const matter = use(MatterContext);
  return { ...matter, paper: (id) => paperIn(matter, id), cite: (id) => citeIn(matter, id) };
}
