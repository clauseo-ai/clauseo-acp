// Whether the Safe workspace (UC) shows: in the settings window's first load, beside its entry, while its body loads
// lazily (workspace-load.tsx). Kept apart from store/protection.ts so the first load carries no store.
import type { HostTransport } from "../../transport/host-transport.ts";

/** Whether the transport carries the protection wire: the desktop's does (its host answers `command.invalid` until
 * R17 PR-B admits this window, and the section says so quietly); a fake only when its test scripts one. */
export const carriesProtectionWire = (transport: HostTransport): boolean =>
  (transport as { protectionWire?: unknown }).protectionWire === true;
