// The room's start, before its first render: the window's display (display/connect.ts, shared with settings.html), and
// in dev and e2e builds the hooks for checks by hand (dev-hooks.ts).
import { late } from "@clauseo/components/lib/late.ts";
import { connectDisplay } from "../display/connect.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { DisplayStore } from "./display-store.ts";
import type { UiSettings } from "./host-next.ts";

let connected: { transport: HostTransport; store: DisplayStore } | null = null;

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
  const { store } = connectDisplay(transport, { override });
  connected = { transport, store };
  // its own chunk, so a release build's initial JS doesn't carry it
  if (HAND_CHECKS) {
    late(import("./dev-hooks.ts")).then(
      (hooks) => hooks.installDevHooks(store),
      (error: unknown) => console.warn(`the dev hooks did not load: ${String(error)}`),
    );
  }
  return store;
}
