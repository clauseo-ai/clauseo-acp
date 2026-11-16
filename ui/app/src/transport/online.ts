// What the window says of the network, for the lines that say why a download waits: `navigator.onLine`, followed
// through the page's `online` / `offline` events (network.ts's signal reads the same). The host's
// `agent.install_offline` says only that the download waits; the window online meanwhile means the download's address
// can't be reached, not that the computer is offline, so the lines say that instead. A test or the review page gives
// its own source through OnlineSource; left out, the page's own.
import { createContext, useCallback, useContext, useSyncExternalStore } from "react";
import { pageSource, type NetworkSource } from "./network.ts";

/** The network the window's lines follow: null, the page's window and navigator. */
export const OnlineSource = createContext<NetworkSource | null>(null);

const still = { addEventListener: () => {}, removeEventListener: () => {} };

/** A network that never changes, for the review page's scenes: online or offline throughout. */
export const FIXED_ONLINE: NetworkSource = { events: still, onLine: () => true };
export const FIXED_OFFLINE: NetworkSource = { events: still, onLine: () => false };

/** Whether the window says it is online, as it changes. */
export function useOnline(): boolean {
  const given = useContext(OnlineSource);
  const subscribe = useCallback(
    (heard: () => void) => {
      const source = given ?? pageSource();
      source.events.addEventListener("online", heard);
      source.events.addEventListener("offline", heard);
      return () => {
        source.events.removeEventListener("online", heard);
        source.events.removeEventListener("offline", heard);
      };
    },
    [given],
  );
  const read = useCallback(() => (given ?? pageSource()).onLine(), [given]);
  return useSyncExternalStore(subscribe, read, read);
}

/** A network a test turns on and off: `set` changes what it reads and fires the page's event for it. */
export function fakeOnline(online: boolean): NetworkSource & { set: (online: boolean) => void } {
  const events = new EventTarget();
  let now = online;
  return {
    events,
    onLine: () => now,
    set: (next) => {
      now = next;
      events.dispatchEvent(new Event(next ? "online" : "offline"));
    },
  };
}
