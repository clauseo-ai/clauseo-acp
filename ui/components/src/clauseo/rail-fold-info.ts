// Where an open fold's info icon stands (U16b): the slot after its name, in its heading's row, which Fold
// (rail-fold.tsx) gives and RailInfo (rail-info.tsx) fills.
import { createContext } from "react";

export const FoldInfo = createContext<HTMLElement | null>(null);
