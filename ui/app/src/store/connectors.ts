// The connectors' state in a window (U15): the latest `ConnectorsState` from the app feed's `connectors_changed`
// (R24), which the host sends whenever it changes and again in every catch-up, so a window that subscribes, or
// subscribes again after its channel reopened, hears the state at once. One store per transport, for either window:
// the settings window's Connectors section reads it, and the room's hint and banner will (U15's room part). The store
// sends nothing: each window's commands live with that window (settings/connector-commands.ts for Settings), so the
// room has no way here to send a settings-only command. No timers: the host publishes, the store hears.
//
//   const store = connectorsStoreOf(transport)   one store per transport, listening from the first ask
//   useConnectors(store)                         the state; NO_CONNECTORS until the first one arrives
//   useConnector(store, id)                      one connector's view, undefined while it isn't listed
//   store.heard()                                whether any state has arrived yet
import { useCallback, useSyncExternalStore } from "react";
import { nextHostOf, type AppEvent } from "../room/host-next.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { ConnectorView, ConnectorsState } from "../transport/protocol.ts";

/** The one connector at launch, Indian Kanoon through Clauseo's case-law service. */
export const CASE_LAW = "case-law";

/** Before any state: nothing listed, so nothing shows. */
export const NO_CONNECTORS: ConnectorsState = { available: false, locked: false, connectors: [] };

export type ConnectorsStore = {
  /** Hears each change; returns the unsubscribe. */
  subscribe: (listener: () => void) => () => void;
  /** The latest state: the same object until one that differs by value arrives. */
  state: () => ConnectorsState;
  /** Whether a state has arrived since the store was made. */
  heard: () => boolean;
};

/** A store over a transport's app feed; a transport with none (NullTransport) gives a store that never hears. */
export function createConnectorsStore(transport: HostTransport): ConnectorsStore {
  let state = NO_CONNECTORS;
  let key = "";
  let heard = false;
  const listeners = new Set<() => void>();
  const hear = (event: AppEvent) => {
    if (!("connectors_changed" in event)) return;
    const next = event.connectors_changed;
    const nextKey = JSON.stringify(next);
    const first = !heard;
    heard = true;
    // a repeat (the catch-up after a reconnect, a balance read that changed nothing) keeps the object
    if (nextKey === key && !first) return;
    key = nextKey;
    state = next;
    for (const listener of listeners) listener();
  };
  nextHostOf(transport).app?.subscribeApp(hear);
  return {
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    state: () => state,
    heard: () => heard,
  };
}

const stores = new WeakMap<HostTransport, ConnectorsStore>();

/** The transport's one connectors store, made (and listening) on first ask. */
export function connectorsStoreOf(transport: HostTransport): ConnectorsStore {
  let store = stores.get(transport);
  if (store === undefined) {
    store = createConnectorsStore(transport);
    stores.set(transport, store);
  }
  return store;
}

/** The state as it changes. */
export function useConnectors(store: ConnectorsStore): ConnectorsState {
  return useSyncExternalStore(store.subscribe, store.state, store.state);
}

/** Whether a state has arrived, as it changes. */
export function useConnectorsHeard(store: ConnectorsStore): boolean {
  return useSyncExternalStore(store.subscribe, store.heard, store.heard);
}

/** One connector's view while the state lists it. */
export function useConnector(store: ConnectorsStore, id: string): ConnectorView | undefined {
  const get = useCallback(() => store.state().connectors.find((c) => c.id === id), [store, id]);
  return useSyncExternalStore(store.subscribe, get, get);
}
