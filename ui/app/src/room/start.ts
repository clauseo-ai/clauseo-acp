// The room's start, before its first render: its display settings on <html> (display-store.ts), data-frame="own" on
// <html> for the frame contract (responsive.css), the host's OS, and in dev and e2e builds the hooks for checks by hand
// (dev-hooks.ts).
import { showDisplay, startDisplay } from "@clauseo/components/lib/display.ts";
import { guessPlatform, hostPlatform } from "../platform.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import { createDisplayStore, type DisplayStore } from "./display-store.ts";
import { nextHostOf, type UiSettings } from "./host-next.ts";
import { late } from "./late.ts";

let connected: { transport: HostTransport; store: DisplayStore } | null = null;

/** localStorage, or null where reaching it throws (storage turned off). */
function pageStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

/** Whether this build carries the hooks for checks by hand: the dev server, or a build made with
 * VITE_CLAUSEO_HAND_CHECKS=1 (the room's e2e build, the probe build). Settled when the build is made, so any other
 * build has neither the hooks' chunk nor axe-core's: never a release build. Not under vitest (mode "test"), where DEV
 * is true too: no test uses them, and they would load in every file that mounts the room. */
const HAND_CHECKS =
  (import.meta.env.DEV && import.meta.env.MODE !== "test") ||
  import.meta.env.VITE_CLAUSEO_HAND_CHECKS === "1";

/** Starts the room's display for a transport, once: a second call with the same transport (Strict Mode's second
 * render) gives the same store; another transport replaces it. `override` holds settings for this page only, never
 * written: the probe build's query. */
export function connectRoom(
  transport: HostTransport,
  override?: Partial<UiSettings>,
): DisplayStore {
  if (connected?.transport === transport) return connected.store;
  connected?.store.dispose();
  const store = createDisplayStore({
    storage: pageStorage(),
    matchMedia: typeof matchMedia === "function" ? (query) => matchMedia(query) : null,
    host: nextHostOf(transport).settings,
    start: startDisplay,
    show: showDisplay,
    // the first frame lays out for the webview's own OS; display_info confirms it or corrects it
    platform: guessPlatform(),
    ...(override === undefined ? {} : { override }),
  });
  connected = { transport, store };
  // the room draws its own frame, the toolbar as the title bar: responsive.css's frame contract keys on this
  document.documentElement.dataset.frame = "own";
  // The window's chrome follows the OS the host reports; nothing on the page waits for it.
  hostPlatform(transport).then(
    (platform) => store.setPlatform(platform),
    (error: unknown) => console.warn(`display_info failed: ${JSON.stringify(error)}`),
  );
  // its own chunk, so a release build's initial JS doesn't carry it
  if (HAND_CHECKS) {
    late(import("./dev-hooks.ts")).then(
      (hooks) => hooks.installDevHooks(store),
      (error: unknown) => console.warn(`the dev hooks did not load: ${String(error)}`),
    );
  }
  return store;
}
