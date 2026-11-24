// The desktop's permissions commands for the first screen's step and Settings' Permissions block (issue 5): stage E's
// permissions() and ask_permission(id) in the desktop crate, through the generated bindings, and the matters-folder
// choice's set_default_location host command. Granted to the main and Settings windows alike
// (crates/desktop/capabilities/settings.json). Here because Tauri APIs are imported only under src/transport/.
import {
  commands,
  type OsPermissionState as BoundState,
  type PermissionId,
  type PermissionRow as BoundRow,
} from "./bindings.ts";
import { toHostError, type HostTransport } from "./host-transport.ts";
import type { FolderRefId } from "./protocol.ts";

export type { PermissionId } from "./bindings.ts";

// TODO(#1344): stage E's bindings add "asking" and `asked` (6b23a56c7); then re-export the bindings' own two types
// in place of these and delete them.
/** Where a permission stands; "asking" while the OS's dialog may be up (the keychain Ask's child runs). Not
 * `PermissionState`: protocol's (an agent's permission request) has that name. */
export type OsPermissionState = BoundState | "asking";

/** One row of the step. `asked`: this build has asked for it, so a keychain row that would still ask is worded
 * "choose Always Allow"; optional here only, so the UI's fixtures stay valid. */
export type PermissionRow = Omit<BoundRow, "state"> & { state: OsPermissionState; asked?: boolean };

/** One command's answer as its data; an error rejects as a HostError. */
async function settled<T>(
  answer: Promise<{ status: "ok"; data: T } | { status: "error"; error: unknown }>,
): Promise<T> {
  let got;
  try {
    got = await answer;
  } catch (error) {
    throw toHostError(error);
  }
  if (got.status === "ok") return got.data;
  throw toHostError(got.error);
}

/** `permissions`: the rows needed on this build. */
export function permissionsCommand(): Promise<PermissionRow[]> {
  return settled(commands.permissions());
}

/** `ask_permission`: asks for one listed row (macOS's own dialog, answered when the lawyer answers), the row after.
 * No folder: only a pick-time id takes one (transport/folder-permission.ts). */
export function askPermissionCommand(id: PermissionId): Promise<PermissionRow> {
  return settled(commands.askPermission(id, null));
}

/** `{"set_default_location": {"folder"}}`: new matters go in `folder` (a pick_folder ref) from now on. Existing
 * matters stay, and nothing on disk is made or moved. Refusals: settings.locked, command.invalid, store.io. */
export async function setDefaultLocationCommand(
  transport: HostTransport,
  folder: FolderRefId,
): Promise<void> {
  await transport.dispatch({
    command_id: crypto.randomUUID(),
    command: { set_default_location: { folder } },
  });
}
