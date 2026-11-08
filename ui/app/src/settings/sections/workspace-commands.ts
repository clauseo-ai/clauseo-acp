// UC: the settings window's two protection commands, the set-up (`retry_protection`) and its removal
// (`remove_sandbox_setup`), which R17 PR-B admits from this window (Ruled, 7). Each runs through protection-run.ts, so
// the window waits on one at a time; the room has only the set-up (room/protection/commands.ts).
import { runProtection } from "../../store/protection-run.ts";
import type { ProtectionStore } from "../../store/protection.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { HostCommand } from "../../transport/protocol.ts";

/** The protection commands Settings sends. */
export type SettingsProtectionCommand = Extract<
  HostCommand,
  { retry_protection: unknown } | { remove_sandbox_setup: unknown }
>;

const send = (transport: HostTransport, command: SettingsProtectionCommand) =>
  transport.dispatch({ command_id: crypto.randomUUID(), command });

/** Sets protection up again, unless a protection command already runs here. */
export const setUpAgain = (transport: HostTransport, store: ProtectionStore): Promise<void> =>
  runProtection(store, "set_up", () => send(transport, { retry_protection: {} }));

/** Removes protection's set-up, after its confirm, unless a protection command already runs here. */
export const removeSetUp = (transport: HostTransport, store: ProtectionStore): Promise<void> =>
  runProtection(store, "remove", () => send(transport, { remove_sandbox_setup: {} }));
