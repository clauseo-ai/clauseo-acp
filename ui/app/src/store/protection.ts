// UC: protection in a window (decision record 0051). Each matter's `Protection`, as `ListMatters`
// and `matters_changed` give it and `protection_changed` changes it; this computer's
// `ProtectionView` from the `protection` query, read when the store is made (a window opening),
// after each protection command answers and on each `protection_changed`, and replaced whole by R17
// PR-B's `protection_view_changed` (read loosely: it isn't in bindings.ts yet). The view is null
// while the query is refused (`command.invalid` before PR-B admits it), which hides or quiets every
// line that needs it. The updater's view (store/update.ts, the window's one read of `get_update`) gives who
// manages this install, which tells a per-machine install from a per-user one for design B's repair;
// followed from the update store, never read twice. One store per
// transport, for either window. It sends no command: each window's live with it (room/protection/
// commands.ts, settings/sections/workspace-commands.ts), run through protection-run.ts, which marks
// `running` here. No timers and no polling: the host publishes, the store hears. Design B's in-app
// repair (room/protection/repair.ts) keeps its run here too, beside display_info's word on it (read
// once, when the store is made), so the room's note and the first run's step share one run.
import { nextHostOf, type AppEvent } from "../room/host-next.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type {
  ManagedBy,
  ProtectionView,
  RepairNote,
  RepairStep,
  UpdateView,
} from "../transport/protocol.ts";
import { updateStoreOf } from "./update.ts";

/** A protection command running from this window: the one-time set-up (`retry_protection`) or its removal. */
export type ProtectionAct = "set_up" | "remove";

/** Design B's in-app repair, "Download and run the installer again" (room/protection/repair.ts). */
export type RepairState = {
  /** display_info's repair_available: a per-user Windows install whose build can fetch its own
   * installer. False until read, and in a window with no repair (Settings). */
  available: boolean;
  /** display_info's repair: this start follows the installer a repair ran, or the installer didn't
   * start and the window was built again. */
  note: RepairNote | null;
  /** The run from this window: "starting" from the press to the channel's first step, then its
   * steps; null when none runs. */
  step: "starting" | RepairStep | null;
  /** The last run's refusal code (launch_failed's after a rebuilt window), until the next starts. */
  failed: string | null;
  /** Whether this run has already brought the lawyer to the card or the set-up after `note`. */
  landed: boolean;
};

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
  /** Who manages this install, by the updater's view: undefined until read, null for nobody else (a
   * per-user install) or while the query is refused. */
  managedBy: ManagedBy | null | undefined;
  repair: RepairState;
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
  managedBy: undefined,
  repair: { available: false, note: null, step: null, failed: null, landed: false },
};

/** The repair's code for an installer that didn't start. */
const LAUNCH_FAILED = "repair.launch_failed";

/** The updater's `managed.by`, or null when the updater manages itself. */
const managedOf = (update: UpdateView | undefined): ManagedBy | null => {
  const state = update?.state;
  return typeof state === "object" && state !== null && "managed" in state
    ? (state.managed?.by ?? null)
    : null;
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
  // the update store's view: undefined until it answers; refused or not scripted, a per-user install (null)
  const followManaged = (): void => {
    const update = updateStoreOf(transport);
    const sync = () => {
      const view = update.state().view;
      const managedBy = view === undefined ? undefined : managedOf(view ?? undefined);
      if (managedBy !== state.managedBy) set({ managedBy });
    };
    update.subscribe(sync);
    sync();
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
  // display_info's word on the repair, once: a launch that failed leaves its code's words on the card
  const readRepair = (): void => {
    transport.displayInfo().then(
      ({ repair_available, repair }) =>
        set({
          repair: {
            ...state.repair,
            available: repair_available === true,
            note: repair ?? null,
            failed: state.repair.failed ?? (repair === "launch_failed" ? LAUNCH_FAILED : null),
          },
        }),
      () => {},
    );
  };
  nextHostOf(transport).app?.subscribeApp(hear);
  // display_info's repair_available follows the updater's state once the host has started (desktop never waits for
  // it there), so it is asked after the first protection read, which the host answers
  const first = read();
  if (wired) {
    followManaged();
    void first.then(readRepair);
  }
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
