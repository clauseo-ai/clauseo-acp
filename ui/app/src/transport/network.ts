// The webview's network signal to the host (`set_network`). The first mount sends what `navigator.onLine` reads, online
// too: the host outlives a webview reload and may hold an old value. After that the window sends only a change: the
// page's `offline` / `online` events, and `navigator.onLine` read again as a turn starts (the window issuing
// `send_prompt`, heard through the transport's `onDispatch`), in case no event fired. While offline, the host ends a
// turn whose agent stays silent for 30 s.
// App mounts the hook once, in the main window.
import { useEffect } from "react";
import type { HostTransport } from "./host-transport.ts";

/** The last value sent to each transport's host; none yet means unknown, so the first mount sends what it reads: the
 * host outlives a webview reload and may still hold an old "offline". Kept per transport, not per mount, so a remount
 * (StrictMode's included) does not send again. */
const lastSent = new WeakMap<HostTransport, boolean>();

/** Where the signal comes from: the page's window and navigator, or a test's. */
export type NetworkSource = {
  events: Pick<Window, "addEventListener" | "removeEventListener">;
  onLine: () => boolean;
};

const pageSource = (): NetworkSource => ({ events: window, onLine: () => navigator.onLine });

/** Watches the network for `transport`'s host and sends `set_network` on each change. Returns the stop. */
export function watchNetwork(
  transport: HostTransport,
  source: NetworkSource = pageSource(),
): () => void {
  const report = (online: boolean) => {
    if (lastSent.get(transport) === online) return;
    lastSent.set(transport, online);
    transport
      .dispatch({ command_id: crypto.randomUUID(), command: { set_network: { online } } })
      .catch((error: unknown) => console.debug(`set_network failed: ${JSON.stringify(error)}`));
  };
  const offline = () => report(false);
  const online = () => report(true);
  source.events.addEventListener("offline", offline);
  source.events.addEventListener("online", online);
  // A turn starts: read the network again, before the prompt's own call goes out (onDispatch runs ahead of it).
  const stopDispatch = transport.onDispatch((envelope) => {
    const body = envelope.command;
    if (typeof body === "object" && body.send_prompt !== undefined) report(source.onLine());
  });
  report(source.onLine());
  return () => {
    source.events.removeEventListener("offline", offline);
    source.events.removeEventListener("online", online);
    stopDispatch();
  };
}

/** Mounts `watchNetwork` for the page's window while the component lives. Mount once, at the app's root. */
export function useNetworkSignal(transport: HostTransport): void {
  useEffect(() => watchNetwork(transport), [transport]);
}
