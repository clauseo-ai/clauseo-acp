// The unsafe-site list's notices in a window (0053): each `site_blocked` the app feed carries, by
// its `block_id`, in memory for the window's run only (the event is never logged and no catch-up
// carries it, so a restart or a reopened window has none). A re-sent event with the same
// `block_id` replaces the site's fields (its higher `count`) and keeps what the lawyer did, so the
// conversation shows one notice per site per turn, updated in place. A notice is shown under its
// turn's answer, so one without a thread or a turn is kept but shown nowhere.
//
// It also holds each notice's allow: sending, allowed (and how far, and whether it was saved), or
// refused with a code. The
// store sends nothing; the room's allow lives in room/blocklist/allow.ts, which nothing in the
// settings window imports.
//
//   const store = blockedSitesOf(transport)    one store per transport, listening from first ask
//   useBlockedSites(store, thread, turn)       the turn's notices, in the order they came
//   store.set(blockId, patch)                  the allow's progress
import { useCallback, useSyncExternalStore } from "react";
import { nextHostOf, type AppEvent, type SiteBlocked } from "../room/host-next.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { AllowScope, BlockedSiteId } from "../transport/protocol.ts";

/** One notice: the site as the host last sent it, and the lawyer's allow. */
export type BlockedNotice = {
  site: SiteBlocked;
  /** While the allow is with the host. */
  sending: boolean;
  /** How far the allow reaches, once the host took it. */
  allowed: AllowScope | null;
  /** The code the host refused the allow with. */
  refused: string | null;
  /** The allow took, but Clauseo couldn't save it (`blocklist.allow_not_saved`): it lasts until
   * Clauseo closes. */
  unsaved: boolean;
};

export type BlockedSitesStore = {
  subscribe: (listener: () => void) => () => void;
  /** A turn's notices, in the order they came: the same array until one of them changes. */
  notices: (thread: string, turn: string) => readonly BlockedNotice[];
  /** One notice, by its id. */
  notice: (blockId: BlockedSiteId) => BlockedNotice | undefined;
  /** Changes a notice's allow. */
  set: (blockId: BlockedSiteId, patch: Partial<Omit<BlockedNotice, "site">>) => void;
};

const NONE: readonly BlockedNotice[] = [];
const keyOf = (thread: string, turn: string) => `${thread} ${turn}`;

/** A store over a transport's app feed; a transport with none gives a store that never hears. */
export function createBlockedSitesStore(transport: HostTransport): BlockedSitesStore {
  const byId = new Map<BlockedSiteId, BlockedNotice>();
  // each turn's ids in order, and its array as last handed out
  const byTurn = new Map<string, BlockedSiteId[]>();
  const shown = new Map<string, readonly BlockedNotice[]>();
  const listeners = new Set<() => void>();
  const changed = (notice: BlockedNotice) => {
    const { thread, turn } = notice.site;
    if (thread !== undefined && turn !== undefined) shown.delete(keyOf(thread, turn));
    for (const listener of listeners) listener();
  };
  const hear = (event: AppEvent) => {
    if (!("site_blocked" in event)) return;
    const site = event.site_blocked;
    const was = byId.get(site.block_id);
    const notice: BlockedNotice =
      was === undefined
        ? { site, sending: false, allowed: null, refused: null, unsaved: false }
        : { ...was, site: { ...site, count: Math.max(site.count, was.site.count) } };
    byId.set(site.block_id, notice);
    if (was === undefined && site.thread !== undefined && site.turn !== undefined) {
      const key = keyOf(site.thread, site.turn);
      byTurn.set(key, [...(byTurn.get(key) ?? []), site.block_id]);
    }
    changed(notice);
  };
  nextHostOf(transport).app?.subscribeApp(hear);
  return {
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    notices: (thread, turn) => {
      const key = keyOf(thread, turn);
      let list = shown.get(key);
      if (list === undefined) {
        const ids = byTurn.get(key);
        list = ids === undefined ? NONE : ids.flatMap((id) => byId.get(id) ?? []);
        shown.set(key, list);
      }
      return list;
    },
    notice: (blockId) => byId.get(blockId),
    set: (blockId, patch) => {
      const was = byId.get(blockId);
      if (was === undefined) return;
      const notice = { ...was, ...patch };
      byId.set(blockId, notice);
      changed(notice);
    },
  };
}

const stores = new WeakMap<HostTransport, BlockedSitesStore>();

/** The transport's one store, made (and listening) on first ask. */
export function blockedSitesOf(transport: HostTransport): BlockedSitesStore {
  let store = stores.get(transport);
  if (store === undefined) {
    store = createBlockedSitesStore(transport);
    stores.set(transport, store);
  }
  return store;
}

/** A turn's notices as they change; none without a turn. */
export function useBlockedSites(
  store: BlockedSitesStore,
  thread: string,
  turn: string | null,
): readonly BlockedNotice[] {
  const get = useCallback(
    () => (turn === null ? NONE : store.notices(thread, turn)),
    [store, thread, turn],
  );
  return useSyncExternalStore(store.subscribe, get, get);
}
