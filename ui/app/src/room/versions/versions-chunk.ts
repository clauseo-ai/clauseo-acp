// Versions' chunk: what the room loads the first time the lawyer opens Versions or presses Undo, with its own sheet.
// The room's first load carries only use-versions.tsx, which imports this with a bare import() (vite.config.ts's
// BARE_CHUNKS): every module it shares with the room is in the first load already.
import sheet from "./versions.css?url";

export { VersionsController } from "./controller.ts";
export { VersionsCentre, VersionsRail } from "./versions-parts.tsx";

let loading: Promise<void> | null = null;

/** Versions' sheet as a stylesheet link from the app's own origin (the CSP's style-src 'self'); resolves once it has
 * loaded, or failed, so the parts never draw unstyled for a frame. Tests, which draw no styles, load none. */
export function loadSheet(): Promise<void> {
  if (loading !== null) return loading;
  loading =
    import.meta.env.MODE === "test"
      ? Promise.resolve()
      : new Promise<void>((resolve) => {
          const link = document.createElement("link");
          link.rel = "stylesheet";
          link.href = sheet;
          link.addEventListener("load", () => resolve(), { once: true });
          link.addEventListener(
            "error",
            () => {
              console.warn("Versions' stylesheet did not load");
              resolve();
            },
            { once: true },
          );
          document.head.append(link);
        });
  return loading;
}
