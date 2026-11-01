// The hooks for the by-hand checks (U1's done-when 6), in dev and e2e builds only (start.ts loads this chunk only
// there): the display from the page's query, `window.__clauseoDisplay(patch)` to change it, and `window.__clauseoAxe()`
// to run axe-core over the page, loaded on call.
import type { AxeResults } from "axe-core";
import { displayFromQuery } from "../gallery/query.ts";
import type { DisplayStore } from "./display-store.ts";
import type { UiSettings } from "./host-next.ts";

declare global {
  interface Window {
    /** Changes the display settings as the lawyer would (display-store.ts's set): theme "system" | "light" | "dark",
     * font_size 14 to 20, language "en" | "hi", lite "automatic" | "full" | "lite", motion "system" | "full" |
     * "reduced". */
    __clauseoDisplay?: (patch: Partial<UiSettings>) => void;
    /** Runs axe-core over the page and gives its violations, colour contrast included. */
    __clauseoAxe?: () => Promise<AxeResults["violations"]>;
  }
}

/** The settings a page's query names (gallery/query.ts's keys: theme=night, lang=hi, lite=1, motion=reduced, size=18),
 * only the keys it has; a query without them leaves the settings alone. */
export function queryPatch(search: string): Partial<UiSettings> {
  const q = new URLSearchParams(search);
  const d = displayFromQuery(search);
  const patch: Partial<UiSettings> = {};
  if (q.has("theme")) patch.theme = d.theme === "night" ? "dark" : "light";
  if (q.has("lang")) patch.language = d.lang;
  if (q.has("lite")) patch.lite = d.render === "lite" ? "lite" : "full";
  if (q.has("motion")) patch.motion = d.motion;
  if (q.has("size")) patch.font_size = d.textSize;
  return patch;
}

/** Applies the page's query for this page only, and sets the two window hooks. */
export function installDevHooks(store: DisplayStore): void {
  const patch = queryPatch(location.search);
  if (Object.keys(patch).length > 0) store.override(patch);
  window.__clauseoDisplay = (next) => store.set(next);
  window.__clauseoAxe = async () => {
    const { default: axe } = await import("axe-core");
    const results = await axe.run(document, { resultTypes: ["violations"] });
    return results.violations;
  };
}
