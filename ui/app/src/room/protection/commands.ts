// UC: the room's one protection command, `retry_protection` (the window split): the main window never removes
// protection's set-up, so this module's type has no way to carry `remove_sandbox_setup`; Settings has its own
// (in the settings window's own module). Run through protection-run.ts, so the window waits on it once.
import { runProtection } from "../../store/protection-run.ts";
import type { ProtectionStore } from "../../store/protection.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { HostCommand } from "../../transport/protocol.ts";

/** The only protection command the room may send. */
export type RoomProtectionCommand = Extract<HostCommand, { retry_protection: unknown }>;

const RETRY: RoomProtectionCommand = { retry_protection: {} };

// @ts-expect-error the room has no typed way to send remove_sandbox_setup (Settings' alone)
const NOT_HERE: RoomProtectionCommand = { remove_sandbox_setup: {} };
void NOT_HERE;

/** Sends the one-time set-up (or, for a full pool, more room), unless one already runs from this window. */
export const retryProtection = (transport: HostTransport, store: ProtectionStore): Promise<void> =>
  runProtection(store, "set_up", () =>
    transport.dispatch({ command_id: crypto.randomUUID(), command: RETRY }),
  );
