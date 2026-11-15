// ?layout=… and ?overlay=open on the probe page (probe-main.tsx): the room's two panels as the window keeps them
// (layout-store.ts), set before the room renders. layout=shown, sidebar-hidden, papers-hidden or both-hidden; any other
// value, or none, leaves what the page's storage holds. overlay=open: once the room shows, the sidebar's toggle is
// pressed if the rail isn't shown, which below 1200 px opens the folded rail's panel over the work. Not shipped.
import { writeLayout, type Layout } from "./layout-store.ts";

const LAYOUTS: Record<string, Layout> = {
  shown: { sidebar: true, papers: true },
  "sidebar-hidden": { sidebar: false, papers: true },
  "papers-hidden": { sidebar: true, papers: false },
  "both-hidden": { sidebar: false, papers: false },
};

/** Keeps the layout `layout` names for the room's window, and with `overlay` "open" opens the sidebar once it shows. */
export function probeLayout(layout: string | null, overlay: string | null): void {
  const kept = layout === null ? undefined : LAYOUTS[layout];
  if (kept !== undefined) writeLayout("main", kept);
  if (overlay !== "open") return;
  const open = () => {
    const toggle = document.querySelector<HTMLElement>('[data-testid="rail-toggle"]');
    // the room stands unseen while it decides its first screen (deciding.ts): wait for it, and for the rail's rows
    const ready =
      toggle !== null &&
      document.querySelector(".room-window[data-deciding]") === null &&
      document.querySelector('nav.room-rail [data-testid="matter-row"]') !== null;
    if (!ready) return false;
    watch.disconnect();
    if (toggle.getAttribute("aria-expanded") !== "true") toggle.click();
    return true;
  };
  const watch = new MutationObserver(open);
  watch.observe(document.documentElement, { subtree: true, childList: true, attributes: true });
}
