// UC: one protection command from a window, as the store marks it (store/protection.ts). The host holds the answer
// until the work is done, Windows' prompt included, then answers Done or `sandbox.setup_refused` / `sandbox.setup_failed`;
// the view follows. While one runs the window waits (`running`), so a second press sends nothing; the wait ends on the
// answer or the first `protection_changed`, whichever comes first, and then the view is read again. A declined native
// confirm (`transport.unauthorized`) says nothing new. Each window's own command module calls it: the room's sends only
// the set-up (room/protection/commands.ts), Settings' both (settings/sections/workspace-commands.ts).
import { useSyncExternalStore } from "react";
import { toHostError } from "../transport/host-transport.ts";
import type { ProtectionAct, ProtectionState, ProtectionStore } from "./protection.ts";

/** The store's state as it changes, for the parts outside the first load (the note, the notice, Settings). */
export function useProtection(store: ProtectionStore): ProtectionState {
  return useSyncExternalStore(store.subscribe, store.state, store.state);
}

/** Runs `send` as `act` unless a protection command already runs here; resolves once the view is read again. */
export async function runProtection(
  store: ProtectionStore,
  act: ProtectionAct,
  send: () => Promise<unknown>,
): Promise<void> {
  if (store.state().running !== null) return;
  store.set({ running: act, failed: null });
  try {
    await send();
  } catch (thrown) {
    const code = toHostError(thrown).code;
    if (code !== "transport.unauthorized") store.set({ failed: code });
  }
  if (store.state().running === act) store.set({ running: null });
  await store.read();
}
