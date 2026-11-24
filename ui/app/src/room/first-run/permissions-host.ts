// The first screen's permissions step's host (issue 5): the desktop's two commands (transport/permission-commands.ts)
// on the desktop, the fake a test or the review page provides for its FakeTransport, and nothing elsewhere: a window
// with neither lists no rows, and the step never shows. System Settings' Files and Folders pane is the transport's own.
import type { FolderChoice } from "@clauseo/components/clauseo/first-launch/folder-choice.tsx";
import { toHostError, type HostTransport } from "../../transport/host-transport.ts";
import {
  HAS_SET_DEFAULT_LOCATION,
  askPermissionCommand,
  permissionsCommand,
  setDefaultLocationCommand,
  type PermissionId,
  type PermissionRow,
} from "../../transport/permission-commands.ts";

export type {
  OsPermissionState,
  PermissionId,
  PermissionRow,
} from "../../transport/permission-commands.ts";

/** The step's host. */
export type PermissionsHost = {
  /** The rows needed on this build. */
  list(): Promise<PermissionRow[]>;
  /** Asks for one row; resolves with the row after, once the lawyer has answered macOS. */
  ask(id: PermissionId): Promise<PermissionRow>;
  /** Picks a folder and makes it where new matters go (stage E's set_default_location, still to come): its outcome, or
   * null when the lawyer cancels the picker. Absent until the command exists, and every button for it is hidden. */
  chooseMattersFolder?: (() => Promise<FolderChoice | null>) | undefined;
};

export type { FolderChoice } from "@clauseo/components/clauseo/first-launch/folder-choice.tsx";

/** The picker, then set_default_location with the folder picked. */
async function chooseOver(transport: HostTransport): Promise<FolderChoice | null> {
  const picked = await transport.pickFolder();
  if (picked === null) return null;
  try {
    await setDefaultLocationCommand(transport, picked.folder);
    return { done: picked.display };
  } catch (error) {
    const { code } = toHostError(error);
    return {
      refused:
        code === "settings.locked" || code === "command.invalid" || code === "store.io"
          ? code
          : "other",
    };
  }
}

const provided = new WeakMap<HostTransport, PermissionsHost>();

/** A test's or the review page's fake for `transport`. */
export function providePermissions(transport: HostTransport, host: PermissionsHost): void {
  provided.set(transport, host);
}

const NONE: PermissionsHost = {
  list: () => Promise.resolve([]),
  ask: () => Promise.reject(new Error("no permissions here")),
};

/** The desktop's: its two commands, and the matters-folder choice once set_default_location exists. */
const desktop = (transport: HostTransport): PermissionsHost => ({
  list: permissionsCommand,
  ask: askPermissionCommand,
  ...(HAS_SET_DEFAULT_LOCATION ? { chooseMattersFolder: () => chooseOver(transport) } : {}),
});

/** The step's host for the window's transport. A list that fails lists no rows. */
export function permissionsHostOf(transport: HostTransport): PermissionsHost {
  const host = provided.get(transport) ?? (transport.kind === "tauri" ? desktop(transport) : NONE);
  return {
    list: () => host.list().catch(() => []),
    ask: (id) => host.ask(id),
    ...(host.chooseMattersFolder === undefined
      ? {}
      : { chooseMattersFolder: host.chooseMattersFolder }),
  };
}

/** The folders Windows' Controlled folder access protects by default, under the lawyer's home. */
const PROTECTED = ["documents", "desktop", "pictures"];

/** A path as the comparison reads it: forward slashes, lower case, no slash at either end, and an absolute path under
 * a home (C:\\Users\\<name>, /Users/<name>, /home/<name>) as the parts below it, as `folder_display` writes it. */
function homeRelative(path: string): string {
  const plain = path
    .replaceAll("\\", "/")
    .toLowerCase()
    .replace(/^\/+|\/+$/g, "");
  return plain.replace(/^(?:[a-z]:\/)?(?:users|home)\/[^/]+\/?/, "");
}

/** The Controlled folder access row that blocks a matter whose folder reads `display` (MatterInfo's folder_display),
 * or null: listed (blocking, or unreadable), and the folder inside the row's folder or a folder it protects by
 * default. */
export function blockingRow(
  rows: readonly PermissionRow[],
  display: string | undefined,
): PermissionRow | null {
  if (display === undefined) return null;
  const row = rows.find(
    (r) => r.id === "controlled_folder_access" && (r.state === "blocking" || r.state === "unknown"),
  );
  if (row === undefined) return null;
  const folder = homeRelative(display);
  const inside = (root: string) =>
    root !== "" && (folder === root || folder.startsWith(`${root}/`));
  const listed = row.folder === null ? "" : homeRelative(row.folder);
  return inside(listed) || PROTECTED.some(inside) ? row : null;
}

/** A fake's matters-folder choice over `transport` (its picker and its dispatch), for tests and the review page. */
export const fakeChoiceOver = (transport: HostTransport) => () => chooseOver(transport);
