// Versions' chunk: what the room loads the first time the lawyer opens Versions or presses Undo, with its own sheet.
// The room's first load carries only use-versions.tsx, which imports this with a bare import() (vite.config.ts's
// BARE_CHUNKS): every module it shares with the room is in the first load already.
import sheet from "./versions.css?url";

export { VersionsController } from "./controller.ts";
export { VersionsCentre, VersionsRail } from "./versions-parts.tsx";

let loading: Promise<void> | null = null;

/** Versions' sheet as a stylesheet link from the app's own origin (the CSP's style-src 'self'); resolves once it has
 * loaded, or failed, so the parts never draw unstyled for a frame. A sheet that failed is asked for again at the next
 * call. Tests load none, unless they ask for the sheet itself (`always`). */
export function loadSheet(always = false): Promise<void> {
  if (import.meta.env.MODE === "test" && !always) return Promise.resolve();
  if (loading !== null) return loading;
  loading = new Promise<void>((resolve) => {
    const link = document.createElement("link");
    link.rel = "stylesheet";
    link.href = sheet;
    link.addEventListener("load", () => resolve(), { once: true });
    link.addEventListener(
      "error",
      () => {
        // Versions opens unstyled this once; the next open asks for the sheet again
        console.warn("Versions' stylesheet did not load");
        link.remove();
        loading = null;
        resolve();
      },
      { once: true },
    );
    document.head.append(link);
  });
  return loading;
}
