// A guarded folder's permission, asked when New matter picks it (issue 5, the pick's part): stage E's macOS kinds on
// PickedFolder.permission, and ask_permission(id, folder) for them. Stage E has not written them yet, so this file is
// the one place they are typed and called; once its bindings land, the types below come from "./bindings.ts" and
// askFolderPermissionCommand calls `commands.askPermission(id, folder)`, and nothing else changes. Here because Tauri
// APIs are imported only under src/transport/.
import { invoke } from "@tauri-apps/api/core";
import { toHostError, type HostTransport } from "./host-transport.ts";
import type { FolderRefId } from "./protocol.ts";

// TODO(stage E): these two types are the shape stage E settled (orchestrator r7368), by name; import them from
// "./bindings.ts" once its PR merges. PermissionId there gains these five beside files_documents and the rest.

/** A folder macOS guards apart from Documents: its Desktop, its Downloads, an external drive, a network drive, or a
 * cloud folder (File Provider). Only macOS sets one; always null on Windows. */
export type FolderPermissionId =
  | "files_desktop"
  | "files_downloads"
  | "files_removable"
  | "files_network"
  | "files_cloud";

/** ask_permission's answer for one of them: the row after the lawyer answered macOS. */
export type FolderPermissionRow = {
  id: FolderPermissionId;
  state: "not_asked" | "granted" | "denied" | "would_ask" | "blocking" | "unknown";
  folder: string | null;
  /** Where in System Settings the lawyer changes it. */
  settings_path: string | null;
  note: string | null;
};

const KINDS: readonly string[] = [
  "files_desktop",
  "files_downloads",
  "files_removable",
  "files_network",
  "files_cloud",
];

/** The permission a picked folder needs (stage E's PickedFolder.permission), or null: none, a Windows pick, a host
 * before stage E, or a kind this page does not know. */
export function folderPermissionOf(picked: object): FolderPermissionId | null {
  const permission: unknown = Reflect.get(picked, "permission");
  return typeof permission === "string" && KINDS.includes(permission)
    ? (permission as FolderPermissionId)
    : null;
}

/** `ask_permission(id, folder)`: macOS's own dialog for the folder picked, answered when the lawyer answers. */
export async function askFolderPermissionCommand(
  id: FolderPermissionId,
  folder: FolderRefId,
): Promise<FolderPermissionRow> {
  try {
    return await invoke<FolderPermissionRow>("ask_permission", { id, folder });
  } catch (error) {
    throw toHostError(error);
  }
}

export type AskFolderPermission = (
  id: FolderPermissionId,
  folder: FolderRefId,
) => Promise<FolderPermissionRow>;

const provided = new WeakMap<HostTransport, AskFolderPermission>();

/** A test's or the review page's fake ask for `transport`. */
export function provideFolderPermission(transport: HostTransport, ask: AskFolderPermission): void {
  provided.set(transport, ask);
}

/** The ask for the window's transport: a fake provided, the desktop's command, or null where neither is (no pick
 * there carries a permission). */
export function folderPermissionAskOf(transport: HostTransport): AskFolderPermission | null {
  return (
    provided.get(transport) ?? (transport.kind === "tauri" ? askFolderPermissionCommand : null)
  );
}
