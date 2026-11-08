// The result's block (result.tsx), its reading and its words, loaded on the first "Show the result" (U17): a bare
// dynamic import behind late, as the plan card's is (limits/plan-card-load.ts), so neither the room's first load nor
// the thread's chunk carries the parser.
import { lazy } from "react";
import { late } from "@clauseo/components/lib/late.ts";

/** The block, once its chunk is in. */
export const ResultBlock = lazy(() =>
  late(import("./result.tsx")).then((m) => ({ default: m.ResultBlock })),
);
