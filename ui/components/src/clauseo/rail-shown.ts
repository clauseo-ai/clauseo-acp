// Whether the rail stands open beside the work, so a RailNotice stands at its foot, or folds
// away, so the notice heads the wall instead.
import { useSyncExternalStore } from "react";

const fullQuery = typeof matchMedia === "function" ? matchMedia("(min-width: 1200px)") : null;
const subscribe = (l: () => void) => {
  fullQuery?.addEventListener("change", l);
  return () => fullQuery?.removeEventListener("change", l);
};
const snap = () => fullQuery?.matches ?? true;

/** Whether the rail stands open beside the work, 1200 px and up, so a notice stands at its foot; below, the rail folds
 * away and the notice heads the wall. */
export function useRailShown(): boolean {
  return useSyncExternalStore(subscribe, snap, snap);
}
