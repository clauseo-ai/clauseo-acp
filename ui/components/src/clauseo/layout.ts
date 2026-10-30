// Where the room stands. Wide (1000 px and up): the wall stands beside the work, so cards grow out
// of it and a provenance line can cross the gap. Narrow: one column, cards sit against what was tapped.
import { useSyncExternalStore } from "react";

const query = typeof matchMedia === "function" ? matchMedia("(min-width: 1000px)") : null;
const subscribe = (l: () => void) => {
  query?.addEventListener("change", l);
  return () => query?.removeEventListener("change", l);
};
const snap = () => query?.matches ?? true;

/** Whether the wall stands beside the work. */
export const isWide = snap;
export function useWide(): boolean {
  return useSyncExternalStore(subscribe, snap, snap);
}
