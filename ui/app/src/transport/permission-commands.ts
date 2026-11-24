// The desktop's permissions commands for the first screen's step and Settings' Permissions block (issue 5): stage E's
// permissions() and ask_permission(id) in the desktop crate, through the generated bindings. Granted to the main and
// Settings windows alike (crates/desktop/capabilities/settings.json). Here because Tauri APIs are imported only under
// src/transport/.
import { commands, type PermissionId, type PermissionRow } from "./bindings.ts";
import { toHostError, type HostTransport } from "./host-transport.ts";
import type { CommandEnvelope, FolderRefId } from "./protocol.ts";

export type { OsPermissionState, PermissionId, PermissionRow } from "./bindings.ts";

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

/** `ask_permission`: asks for one row (macOS's own dialog, answered when the lawyer answers), the row after. */
export function askPermissionCommand(id: PermissionId): Promise<PermissionRow> {
  return settled(commands.askPermission(id));
}

/** TODO(set_default_location): true once bindings.ts has the command (stage E's small PR after #1279). Until then
 * every "matters folder" choice that uses it stays hidden. */
export const HAS_SET_DEFAULT_LOCATION = false;

/** `{"set_default_location": {"folder"}}`: new matters go in `folder` (a pick_folder ref) from now on. Existing
 * matters stay, and nothing on disk is made or moved. Refusals: settings.locked, command.invalid, store.io. */
export async function setDefaultLocationCommand(
  transport: HostTransport,
  folder: FolderRefId,
): Promise<void> {
  await transport.dispatch({
    command_id: crypto.randomUUID(),
    command: { set_default_location: { folder } },
  } as unknown as CommandEnvelope);
}
