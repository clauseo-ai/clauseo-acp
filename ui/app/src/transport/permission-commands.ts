// The desktop's permissions commands for the first screen's step (issue 5): stage E's two Tauri commands in the
// desktop crate, permissions() and ask_permission(id), typed with specta in #1279 and not in this branch's bindings.ts.
// Until #1279 merges they are called by name and typed here; after, this file calls `commands.permissions()` and
// `commands.askPermission(id)` and imports the types, and nothing else changes. Here because Tauri APIs are imported only under src/transport/.
import { invoke } from "@tauri-apps/api/core";
import { toHostError, type HostTransport } from "./host-transport.ts";
import type { CommandEnvelope, FolderRefId } from "./protocol.ts";

// TODO(#1279): these three types are bindings.ts's at #1279's head 31ed03eb4, copied by shape and name; once #1279
// merges, import them from "./bindings.ts" in place of this block.

/** A permission the step may list. */
export type PermissionId = "files_documents" | "keychain" | "controlled_folder_access";

/** Where a permission stands. Not `PermissionState`: protocol's (an agent's permission request) has that name. */
export type OsPermissionState =
  | "not_asked"
  | "granted"
  | "denied"
  | "would_ask"
  | "blocking"
  | "unknown";

/** One row of the step. `settings_path` and `note` are shown as given. */
export type PermissionRow = {
  id: PermissionId;
  state: OsPermissionState;
  /** The folder it is about (files_documents: Documents). */
  folder: string | null;
  /** Where in the OS's settings the lawyer changes it, when Clauseo cannot. */
  settings_path: string | null;
  /** More words: the keychain items concerned, or what else helps. */
  note: string | null;
};

/** `permissions`: the rows needed on this build. */
export async function permissionsCommand(): Promise<PermissionRow[]> {
  try {
    return await invoke<PermissionRow[]>("permissions");
  } catch (error) {
    throw toHostError(error);
  }
}

/** `ask_permission`: asks for one row (macOS's own dialog, answered when the lawyer answers), the row after. */
export async function askPermissionCommand(id: PermissionId): Promise<PermissionRow> {
  try {
    return await invoke<PermissionRow>("ask_permission", { id });
  } catch (error) {
    throw toHostError(error);
  }
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
