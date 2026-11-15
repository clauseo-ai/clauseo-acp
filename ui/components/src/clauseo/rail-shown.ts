// Whether the rail stands open beside the work, so a RailNotice stands at its foot, or folds
// away, so the notice heads the wall instead; and whether the room stands in one column, where the wall comes after the
// work and the docked composer, so a notice heads the work instead.
import { useSyncExternalStore } from "react";

function query(media: string, missing: boolean) {
  const list = typeof matchMedia === "function" ? matchMedia(media) : null;
  const subscribe = (l: () => void) => {
    list?.addEventListener("change", l);
    return () => list?.removeEventListener("change", l);
  };
  const snap = () => list?.matches ?? missing;
  return function useMedia(): boolean {
    return useSyncExternalStore(subscribe, snap, snap);
  };
}

/** Whether the rail stands open beside the work, 1200 px and up, so a notice stands at its foot; below, the rail folds
 * away and the notice heads the wall. */
export const useRailShown: () => boolean = query("(min-width: 1200px)", true);

/** Whether the room stands in one column, under 1000 px (responsive.css's max-mid): the wall comes after the work, under
 * the docked composer, so a notice heads the work, where the composer cannot cover it. */
export const useOneColumn: () => boolean = query("(max-width: 999.98px)", false);

/** Where the notices stand: at the rail's foot where the rail stands beside the work (`railFree`: Versions' timeline
 * does not hold its place); at the work's head in one column, where the docked composer would cover the wall's head,
 * and over the first run's welcome, which puts the wall away; else at the wall's head, before the papers. */
export function useNoticePlace(railFree: boolean, welcome: boolean): "rail" | "work" | "wall" {
  const railShown = useRailShown();
  const oneColumn = useOneColumn();
  if (railShown && railFree) return "rail";
  return oneColumn || welcome ? "work" : "wall";
}
