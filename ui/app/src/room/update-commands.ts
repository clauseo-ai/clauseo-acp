// R23: the main window's two update commands (decision record 0037): install the ready version now or after the
// running answer, and take a scheduled install back. The host admits both from the main window only, and this module
// is the room's alone; Settings checks and sets the mode (settings/sections/update-commands.ts).
import { runUpdate, type UpdateStore } from "../store/update.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { InstallWhen } from "../transport/protocol.ts";

/** Installs the ready version: `now` stops running turns first, `after_turn` waits for them (`scheduled`). */
export const installUpdate = (
  transport: HostTransport,
  store: UpdateStore,
  when: InstallWhen,
): Promise<void> =>
  runUpdate(transport, store, when === "now" ? "install_now" : "install_after", {
    install_update: { when },
  });

/** Takes a scheduled install back to ready. */
export const cancelScheduledUpdate = (
  transport: HostTransport,
  store: UpdateStore,
): Promise<void> => runUpdate(transport, store, "cancel", { cancel_scheduled_update: {} });
