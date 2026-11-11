// The tracked changes' chunk: what the room loads the first time the lawyer presses a .docx tile's Tracked changes
// (paper-wall.tsx), with its own sheet, so the room's first load carries none of the panel, its dialog or its words.
// It shares the dialog primitives with New matter's dialog and Versions, so it keeps Vite's preload list
// (vite.config.ts's PRELOADED_CHUNKS).
import sheet from "./changes.css?url";

export { ChangesPane } from "./changes-pane.tsx";

let loading: Promise<void> | null = null;

/** The panel's sheet as a stylesheet link from the app's own origin (the CSP's style-src 'self'); resolves once it has
 * loaded, or failed, so the panel never draws unstyled for a frame. A sheet that failed is asked for again at the next
 * call. Tests load none, as Versions' don't (versions-chunk.ts). */
export function loadSheet(): Promise<void> {
  if (import.meta.env.MODE === "test") return Promise.resolve();
  if (loading !== null) return loading;
  loading = new Promise<void>((resolve) => {
    const link = document.createElement("link");
    link.rel = "stylesheet";
    link.href = sheet;
    link.addEventListener("load", () => resolve(), { once: true });
    link.addEventListener(
      "error",
      () => {
        // the panel opens unstyled this once; the next open asks for the sheet again
        console.warn("the tracked changes' stylesheet did not load");
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
