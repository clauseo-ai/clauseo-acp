// A window's display, before its first render, for every entry that draws Clauseo's own frame (the room's index.html,
// settings.html): its settings on <html> through the one display store (room/display-store.ts), data-frame="own" on
// <html> for the frame contract (responsive.css), and the OS the host reports. Each entry calls it once per page.
import { showDisplay, startDisplay } from "@clauseo/components/lib/display.ts";
import { guessPlatform, hostPlatform } from "../platform.ts";
import { createDisplayStore, type DisplayStore } from "../room/display-store.ts";
import { nextHostOf, type UiSettings } from "../room/host-next.ts";
import type { HostTransport } from "../transport/host-transport.ts";

/** localStorage, or null where reaching it throws (storage turned off). */
function pageStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

export type ConnectOptions = {
  /** Settings for this page only, never written: the probe build's query. */
  override?: Partial<UiSettings> | undefined;
};

/** Starts this window's display for a transport and returns its store. Without a host the store also hears the other
 * window's changes to localStorage (`storage` events); with one, the host's settings_changed reaches both. */
export function connectDisplay(
  transport: HostTransport,
  options: ConnectOptions = {},
): { store: DisplayStore } {
  const store = createDisplayStore({
    storage: pageStorage(),
    matchMedia: typeof matchMedia === "function" ? (query) => matchMedia(query) : null,
    events: typeof window === "undefined" ? null : window,
    host: nextHostOf(transport).settings,
    start: startDisplay,
    show: showDisplay,
    // the first frame lays out for the webview's own OS; display_info confirms it or corrects it
    platform: guessPlatform(),
    ...(options.override === undefined ? {} : { override: options.override }),
  });
  // the window draws its own frame: responsive.css's frame contract keys on this
  document.documentElement.dataset.frame = "own";
  // The window's chrome follows the OS the host reports; nothing on the page waits for it.
  hostPlatform(transport).then(
    (platform) => store.setPlatform(platform),
    (error: unknown) => console.warn(`display_info failed: ${JSON.stringify(error)}`),
  );
  return { store };
}
