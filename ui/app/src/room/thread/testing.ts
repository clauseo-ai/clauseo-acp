// Test helpers for the room's thread under happy-dom. Not shipped: only tests import it.

/** happy-dom has no font loading, and the thread (ui/components' Thread) waits on the fonts once it settles: here the
 * faces count as loaded, as the thread's own tests have them. */
export function stubFonts(): void {
  if (!("fonts" in document)) {
    Object.defineProperty(document, "fonts", {
      value: { ready: Promise.resolve(), status: "loaded", load: async () => [] },
    });
  }
}
