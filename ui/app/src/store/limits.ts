// The limits view in the window (U12): the latest `AgentLimitsView` per agent kind, from the channel's
// `limits_view_changed` (transport/limits-feed.ts), and `refresh_limits` to ask the host for a read. The store changes
// only when a view arrives, and then only for an agent whose entry changed: an unchanged entry keeps its object, so a
// part that reads one agent renders again only when that agent's figures move. No timers: the host publishes a new
// view at each shown reset and at the stale mark (and focus asks for a read), and the parts format their times
// against the moment the last view arrived (`at`), so a render stays pure and a weekday is true as of that view.
//
// For a part (the toolbar's plan, the composer's note, Settings' Plan and usage):
//   const store = limitsStoreOf(transport)          one store per transport, made on first ask
//   <LimitsContext value={store}>…</LimitsContext>  the room (and the settings window) provide it
//   useAgentLimits(kind)                            one agent's entry, undefined before its first view
//   useLimitsView()                                 every agent's entry, keyed by kind
//   useLimitsAt()                                   when the last view arrived: the "now" times are said against
//   store.refresh(kind, "shown" | "try_again")      refresh_limits; undefined kind asks for every agent
import { createContext, use, useCallback, useSyncExternalStore } from "react";
import type { HostTransport } from "../transport/host-transport.ts";
import { isLimitsSource } from "../transport/limits-feed.ts";
import type { AgentKind, AgentLimitsView, LimitsView, RefreshWhy } from "../transport/protocol.ts";

export type LimitsStore = {
  /** Hears each change; returns the unsubscribe. */
  subscribe: (listener: () => void) => () => void;
  /** Every agent's entry, keyed by kind: the same object until a view changes one. */
  view: () => LimitsView;
  /** When the last view that changed anything arrived (ms since the epoch); 0 before the first. */
  at: () => number;
  /** One agent's entry; undefined before a view names it. */
  agent: (kind: AgentKind) => AgentLimitsView | undefined;
  /** Asks the host to read the limits (`refresh_limits`): `shown` reads only figures older than 5 minutes,
   * `try_again` reads now. A refusal goes to the console; the next view says what came of it. */
  refresh: (kind: AgentKind | undefined, why: RefreshWhy) => void;
};

const NONE: LimitsView = {};

/** A store over a transport's limits; a transport that carries none (NullTransport) gives an empty store. */
export function createLimitsStore(transport: HostTransport): LimitsStore {
  let view: LimitsView = NONE;
  let at = 0;
  let keys: Partial<Record<AgentKind, string>> = {};
  const listeners = new Set<() => void>();
  const take = (next: LimitsView) => {
    const kept: LimitsView = {};
    const nextKeys: Partial<Record<AgentKind, string>> = {};
    let changed = Object.keys(next).length !== Object.keys(view).length;
    for (const [kind, entry] of Object.entries(next) as [AgentKind, AgentLimitsView][]) {
      const key = JSON.stringify(entry);
      nextKeys[kind] = key;
      const old = view[kind];
      if (old !== undefined && keys[kind] === key) kept[kind] = old;
      else {
        kept[kind] = entry;
        changed = true;
      }
    }
    keys = nextKeys;
    if (!changed) return;
    view = kept;
    at = Date.now();
    for (const listener of listeners) listener();
  };
  if (isLimitsSource(transport)) transport.onLimits(take);
  return {
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    view: () => view,
    at: () => at,
    agent: (kind) => view[kind],
    refresh: (kind, why) => {
      transport
        .dispatch({
          command_id: crypto.randomUUID(),
          command: { refresh_limits: kind === undefined ? { why } : { agent: kind, why } },
        })
        .catch((error: unknown) => console.warn(`refresh_limits failed: ${JSON.stringify(error)}`));
    },
  };
}

const stores = new WeakMap<HostTransport, LimitsStore>();

/** The transport's one limits store, made on first ask. */
export function limitsStoreOf(transport: HostTransport): LimitsStore {
  let store = stores.get(transport);
  if (store === undefined) {
    store = createLimitsStore(transport);
    stores.set(transport, store);
  }
  return store;
}

/** The window's limits store; null outside a provider, where every hook reads nothing. */
export const LimitsContext = createContext<LimitsStore | null>(null);

const nothing = () => () => {};

/** The store the parts read, or null outside a provider. */
export const useLimitsStore = (): LimitsStore | null => use(LimitsContext);

/** One agent's entry; undefined outside a provider, for no kind, or before a view names it. */
export function useAgentLimits(kind: AgentKind | undefined): AgentLimitsView | undefined {
  const store = use(LimitsContext);
  const get = useCallback(
    () => (store === null || kind === undefined ? undefined : store.agent(kind)),
    [store, kind],
  );
  return useSyncExternalStore(store?.subscribe ?? nothing, get, get);
}

/** Every agent's entry, keyed by kind; empty outside a provider and before the first view. */
export function useLimitsView(): LimitsView {
  const store = use(LimitsContext);
  const get = useCallback(() => store?.view() ?? NONE, [store]);
  return useSyncExternalStore(store?.subscribe ?? nothing, get, get);
}

/** When the last view arrived (ms since the epoch), the moment the parts say their times against; 0 outside a
 * provider and before the first view. */
export function useLimitsAt(): number {
  const store = use(LimitsContext);
  const get = useCallback(() => store?.at() ?? 0, [store]);
  return useSyncExternalStore(store?.subscribe ?? nothing, get, get);
}

/** A value derived from one agent's entry and the last view's arrival, by `select`, which must give a value that
 * compares equal by Object.is while the facts it reads stay the same (a string or a number): the part renders again
 * only when it changes. */
export function useAgentSelect<T>(
  kind: AgentKind | undefined,
  select: (entry: AgentLimitsView | undefined, at: number) => T,
): T {
  const store = use(LimitsContext);
  const get = useCallback(
    () =>
      store === null || kind === undefined
        ? select(undefined, 0)
        : select(store.agent(kind), store.at()),
    [store, kind, select],
  );
  return useSyncExternalStore(store?.subscribe ?? nothing, get, get);
}
