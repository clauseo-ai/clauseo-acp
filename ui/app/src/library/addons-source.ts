// The one place Skills and connectors reads its list from. Every part of the place reads through
// here, so wiring stage D's list_addons is a change to this file alone.
// TODO(stage D, D1: list_addons + addons_changed): make createAddonsSource ask
// `transport.query({ list_addons: {} })`, read again on each `addons_changed` app event and on
// `read()`, and give `recheck` as one more read (draft 2: list_addons checks Case Law's liveness
// again). Then delete addons-interim.ts.
import type { HostTransport } from "../transport/host-transport.ts";
import { useSyncExternalStore } from "react";
import type { AddonView } from "./addons.ts";
import { interimSource } from "./addons-interim.ts";

export type AddonsState = {
  /** The items in the place's order; null until the first answer. */
  items: readonly AddonView[] | null;
};

export type AddonsSource = {
  subscribe: (listener: () => void) => () => void;
  state: () => AddonsState;
  /** Reads the list again: when the place opens. */
  read: () => void;
  /** Asks for Clauseo Case Law's reach to be checked again (its Retry); null where it can't be. */
  recheck: (() => void) | null;
};

/** The list's source over a transport, listening from the moment it is made. */
export function createAddonsSource(transport: HostTransport): AddonsSource {
  return interimSource(transport);
}

const sources = new WeakMap<HostTransport, AddonsSource>();

/** The transport's one source, made on first ask: the room asks at its start, so no catch-up goes
 * unheard. */
export function addonsSourceOf(transport: HostTransport): AddonsSource {
  let source = sources.get(transport);
  if (source === undefined) {
    source = createAddonsSource(transport);
    sources.set(transport, source);
  }
  return source;
}

/** Tests and the copy review: `source` stands in for the transport's own, given before the room
 * mounts. */
export function provideAddons(transport: HostTransport, source: AddonsSource): void {
  sources.set(transport, source);
}

/** A source that lists `items` and never changes: the review page's and the shots'. */
export function fixedAddons(
  items: readonly AddonView[],
  recheck: (() => void) | null = null,
): AddonsSource {
  const state: AddonsState = { items };
  return { subscribe: () => () => {}, state: () => state, read: () => {}, recheck };
}

/** The list as it changes. */
export function useAddons(source: AddonsSource): AddonsState {
  return useSyncExternalStore(source.subscribe, source.state, source.state);
}
