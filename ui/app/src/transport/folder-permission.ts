// A guarded folder's permission, asked when New matter picks it (issue 5, the pick's part): stage E's macOS kinds on
// PickedFolder.permission, and `commands.askPermission(id, folder)` for them (desktop permissions/pick.rs). Here
// because Tauri APIs are imported only under src/transport/.
import { commands, type PermissionId, type PermissionRow } from "./bindings.ts";
import { toHostError, type HostTransport } from "./host-transport.ts";
import type { FolderRefId, PickedFolder } from "./protocol.ts";

/** A folder macOS guards apart from Documents: its Desktop, its Downloads, an external drive, a network drive, or a
 * cloud folder (File Provider). Only macOS sets one; always null on Windows. The pick-time ids of PermissionId. */
export type FolderPermissionId = Extract<
  PermissionId,
  "files_desktop" | "files_downloads" | "files_removable" | "files_network" | "files_cloud"
>;

/** ask_permission's answer for one of them: the row after the lawyer answered macOS, `settings_path` saying where in
 * System Settings it changes. */
export type FolderPermissionRow = PermissionRow;

const KINDS: readonly PermissionId[] = [
  "files_desktop",
  "files_downloads",
  "files_removable",
  "files_network",
  "files_cloud",
] satisfies FolderPermissionId[];

/** The permission a picked folder needs (PickedFolder.permission), or null: none, a Windows pick, or a kind this page
 * does not know. */
export function folderPermissionOf(
  picked: Pick<PickedFolder, "permission">,
): FolderPermissionId | null {
  const { permission } = picked;
  return permission != null && KINDS.includes(permission)
    ? (permission as FolderPermissionId)
    : null;
}

/** `ask_permission(id, folder)`: macOS's own dialog for the folder picked, answered when the lawyer answers. */
export async function askFolderPermissionCommand(
  id: FolderPermissionId,
  folder: FolderRefId,
): Promise<FolderPermissionRow> {
  let answer: Awaited<ReturnType<typeof commands.askPermission>>;
  try {
    answer = await commands.askPermission(id, folder);
  } catch (error) {
    throw toHostError(error);
  }
  if (answer.status === "ok") return answer.data;
  throw toHostError(answer.error);
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
