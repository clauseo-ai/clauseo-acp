// R23: the updater in a window (decision record 0037). The host's UpdateView, read with `get_update` when the store is
// made (a window opening) and replaced whole by each `update_changed` on the app feed: the one place a window reads it,
// so the room's notice, Settings' Updates and the protection store's `managedBy` (store/protection.ts) share one read
// and one listener. The view is undefined until the first answer and null while the query is refused or the transport
// has no updater, which shows nothing. Each window sends its own commands through `runUpdate`: the room installs and
// cancels a scheduled install (room/update-commands.ts), Settings checks and sets the mode
// (settings/sections/update-commands.ts), as the host admits them. No timers and no polling: the host publishes.
import { useSyncExternalStore } from "react";
import { nextHostOf, type AppEvent } from "../room/host-next.ts";
import { toHostError, type HostTransport } from "../transport/host-transport.ts";
import type { HostCommand, UpdateState as Updater, UpdateView } from "../transport/protocol.ts";

/** A command a window sends the updater, by what it does. */
export type UpdateAct = "check" | "install_now" | "install_after" | "cancel" | "mode";

/** The updater's four commands. */
export type UpdateCommand = Extract<
  HostCommand,
  | { check_for_update: unknown }
  | { install_update: unknown }
  | { cancel_scheduled_update: unknown }
  | { set_update_mode: unknown }
>;

export type UpdateStoreState = {
  /** The host's view: undefined until the first answer, null while refused or with no updater. */
  view: UpdateView | null | undefined;
  /** The command this window sent and waits on, until its answer. */
  sending: UpdateAct | null;
  /** The last command's refusal, until the next one starts. */
  refused: { act: UpdateAct; code: string } | null;
};

export type UpdateStore = {
  subscribe: (listener: () => void) => () => void;
  state: () => UpdateStoreState;
  set: (patch: Partial<UpdateStoreState>) => void;
};

const EMPTY: UpdateStoreState = { view: undefined, sending: null, refused: null };

/** Whether the transport carries the updater: the desktop's does, a fake when its test scripts it (`host.update`).
 * Settings' Updates shows only then. */
export const carriesUpdate = (transport: HostTransport): boolean =>
  (transport as { updateWire?: unknown }).updateWire === true;

/** Whether the store reads the view: with the updater, or with the protection wire, whose repair line reads who
 * manages the install from it. */
const readsUpdate = (transport: HostTransport): boolean =>
  carriesUpdate(transport) || (transport as { protectionWire?: unknown }).protectionWire === true;

export function createUpdateStore(transport: HostTransport): UpdateStore {
  let state = EMPTY;
  const listeners = new Set<() => void>();
  const set = (patch: Partial<UpdateStoreState>) => {
    state = { ...state, ...patch };
    for (const listener of listeners) listener();
  };
  if (readsUpdate(transport)) {
    // a pushed view stands over the first read's answer, however late that answer comes
    let pushed = false;
    nextHostOf(transport).app?.subscribeApp((event: AppEvent) => {
      if (!("update_changed" in event)) return;
      pushed = true;
      set({ view: event.update_changed });
    });
    void (async () => {
      let view: UpdateView | null = null;
      try {
        view = (await transport.query("get_update")).update ?? null;
      } catch {
        // refused or not scripted: no updater here, and nothing shows
      }
      if (!pushed) set({ view });
    })();
  } else set({ view: null });
  return {
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    state: () => state,
    set,
  };
}

const stores = new WeakMap<HostTransport, UpdateStore>();

/** The transport's one update store, made (listening, and reading the view) on first ask. */
export function updateStoreOf(transport: HostTransport): UpdateStore {
  let store = stores.get(transport);
  if (store === undefined) {
    store = createUpdateStore(transport);
    stores.set(transport, store);
  }
  return store;
}

/** The store's state as it changes. */
export function useUpdate(store: UpdateStore): UpdateStoreState {
  return useSyncExternalStore(store.subscribe, store.state, store.state);
}

/** Sends `command` as `act` unless an update command already waits here; a refusal's code stays in `refused`. The
 * host's `update_changed` carries what follows. */
export async function runUpdate(
  transport: HostTransport,
  store: UpdateStore,
  act: UpdateAct,
  command: UpdateCommand,
): Promise<void> {
  if (store.state().sending !== null) return;
  store.set({ sending: act, refused: null });
  try {
    await transport.dispatch({ command_id: crypto.randomUUID(), command });
  } catch (thrown) {
    store.set({ refused: { act, code: toHostError(thrown).code } });
  }
  if (store.state().sending === act) store.set({ sending: null });
}

/** The state's name (`"idle"`, or the one key of `{ "ready": { "version": "0.4.1" } }`) and, where it has one, its
 * version. */
export function updaterOf(state: Updater): { name: string; version: string | null } {
  if (typeof state === "string") return { name: state, version: null };
  const [name, body] = Object.entries(state)[0] ?? ["idle", undefined];
  const version = (body as { version?: string | null } | undefined)?.version ?? null;
  return { name, version };
}
