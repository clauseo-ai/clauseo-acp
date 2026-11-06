// UC: protection in a window (decision record 0051). Each matter's `Protection`, as `ListMatters` and `matters_changed`
// give it and `protection_changed` changes it; this computer's `ProtectionView` from the `protection` query, read when
// the store is made (a window opening), after each protection command answers and on each `protection_changed`, and
// replaced whole by R17 PR-B's `protection_view_changed` (read loosely: it isn't in bindings.ts yet). The view is null
// while the query is refused (`command.invalid` before PR-B admits it), which hides or quiets every line that needs it.
// One store per transport, for either window. It sends no command: each window's live with it (room/protection/
// commands.ts, settings/sections/workspace-commands.ts), run through protection-run.ts, which marks `running` here. No
// timers and no polling: the host publishes, the store hears.
import { nextHostOf, type AppEvent } from "../room/host-next.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { ProtectionView } from "../transport/protocol.ts";

/** A protection command running from this window: the one-time set-up (`retry_protection`) or its removal. */
export type ProtectionAct = "set_up" | "remove";

export type ProtectionState = {
  /** This computer's view: undefined until the first answer, null while the query is refused. */
  view: ProtectionView | null | undefined;
  /** The command this window sent and is waiting on, until its answer or the first `protection_changed`. */
  running: ProtectionAct | null;
  /** The last protection command's refusal code, until the next one starts. */
  failed: string | null;
  /** The first run's Safe workspace step (first-run/workspace.ts), for this run: not entered, asking, or with its
   * outcome (set up, refused, failed or Not now). */
  step: "ask" | "done" | null;
};

export type ProtectionStore = {
  subscribe: (listener: () => void) => () => void;
  state: () => ProtectionState;
  set: (patch: Partial<ProtectionState>) => void;
  /** Reads the view again; only the last read asked for sets it. */
  read: () => Promise<void>;
};

const EMPTY: ProtectionState = {
  view: undefined,
  running: null,
  failed: null,
  step: null,
};

/** Whether the transport carries the protection wire: the desktop's does, a fake only when its test scripts it. */
export const carriesProtection = (transport: HostTransport): boolean =>
  (transport as { protectionWire?: unknown }).protectionWire === true;

export function createProtectionStore(transport: HostTransport): ProtectionStore {
  let state = EMPTY;
  const listeners = new Set<() => void>();
  const set = (patch: Partial<ProtectionState>) => {
    state = { ...state, ...patch };
    for (const listener of listeners) listener();
  };
  const wired = carriesProtection(transport);
  // each read's answer stands only while no later read was asked for, so an older answer never replaces a newer one
  let asked = 0;
  const read = async (): Promise<void> => {
    if (!wired) return;
    const n = (asked += 1);
    let view: ProtectionView | null = null;
    try {
      view = (await transport.query("protection")).protection ?? null;
    } catch {
      // refused (command.invalid until R17 PR-B admits the window, or anything else): the quiet state, never an error
    }
    if (n === asked) set({ view });
  };
  const hear = (event: AppEvent) => {
    if ("protection_changed" in event) {
      // the matter's row takes it (matters.ts); the wait ends on the command's answer or the first protection_changed,
      // whichever comes first, and the view is read again
      set({ running: null });
      void read();
    } else if ("protection_view_changed" in event) {
      // R17 PR-B's { view }, read loosely: anything without a computer is ignored
      const view = (event.protection_view_changed as { view?: ProtectionView } | null)?.view;
      if (view?.computer !== undefined) set({ view });
    }
  };
  nextHostOf(transport).app?.subscribeApp(hear);
  void read();
  return {
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    state: () => state,
    set,
    read,
  };
}

const stores = new WeakMap<HostTransport, ProtectionStore>();

/** The transport's one protection store, made (listening, and reading the view) on first ask. */
export function protectionStoreOf(transport: HostTransport): ProtectionStore {
  let store = stores.get(transport);
  if (store === undefined) {
    store = createProtectionStore(transport);
    stores.set(transport, store);
  }
  return store;
}
