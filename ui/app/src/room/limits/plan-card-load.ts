// The plan card's chunk (plan-card-view.tsx), loaded once (U12): the toolbar's plan starts it as the pointer comes over
// the plan or focus reaches it, so the first press finds it loaded or on its way, and the card opens with its windows.
// A bare dynamic import like every other lazy part: no modulepreload, and the card stays out of the first load.
import { late } from "@clauseo/components/lib/late.ts";

type CardModule = typeof import("./plan-card-view.tsx");

let loading: Promise<CardModule> | null = null;

/** Starts loading the card's chunk, once; later calls give back the same import. */
export function preloadPlanCard(): Promise<CardModule> {
  if (loading === null) {
    const load = late(import("./plan-card-view.tsx"));
    // a load that failed is tried again on the next ask
    load.catch(() => {
      if (loading === load) loading = null;
    });
    loading = load;
  }
  return loading;
}
