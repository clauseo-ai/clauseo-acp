// R23: the settings window's two update commands (decision record 0037): check now, from idle or failed, and set
// `update.mode` to auto or notify. The host admits set_update_mode from this window only (a policy lock gives
// settings.locked); installing is the main window's (room/update-commands.ts).
import { runUpdate, type UpdateStore } from "../../store/update.ts";
import type { HostTransport } from "../../transport/host-transport.ts";

/** Asks the host to check for a newer version now. */
export const checkForUpdate = (transport: HostTransport, store: UpdateStore): Promise<void> =>
  runUpdate(transport, store, "check", { check_for_update: {} });

/** Sets how updates install: `auto` at the next start, `notify` only when the lawyer says so. */
export const setUpdateMode = (
  transport: HostTransport,
  store: UpdateStore,
  mode: "auto" | "notify",
): Promise<void> => runUpdate(transport, store, "mode", { set_update_mode: { mode } });
