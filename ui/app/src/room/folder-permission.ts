// A guarded folder's line under New matter's folder (issue 5, the pick's part): after a pick macOS guards apart from
// Documents (its Desktop, its Downloads, an external drive, a network drive, a cloud folder; stage E's
// PickedFolder.permission), one reason line for that kind and Allow, above Create. Nothing is asked until Allow is
// pressed: then ask_permission(id, folder) with the pick's ref raises macOS's own dialog. Granted, the line goes.
// Denied or dismissed, Create still works, and the line says what won't work and where in System Settings to change
// it, with the path the desktop answered. A status, as the synced folder's notice is (scan/place-notice.tsx): New
// matter's error line stays its one alert. Part of New matter's chunk.
// The line itself: folder-permission-line.tsx.
import type { MatterFormKey } from "@clauseo/components/lib/words-matter-form.ts";
import { useRef, useState } from "react";
import { folderPermissionOf, type FolderPermissionId } from "../transport/folder-permission.ts";
import type { FolderRefId, PickedFolder } from "../transport/protocol.ts";
import type { Matters } from "./matters.ts";

/** Each kind's reason line. */
export const REASON: Record<FolderPermissionId, MatterFormKey> = {
  files_desktop: "Clauseo needs macOS's permission to read and save papers in your Desktop folder.",
  files_downloads:
    "Clauseo needs macOS's permission to read and save papers in your Downloads folder.",
  files_removable:
    "Clauseo needs macOS's permission to read and save papers on this external drive.",
  files_network: "Clauseo needs macOS's permission to read and save papers on this network drive.",
  files_cloud: "Clauseo needs macOS's permission to read and save papers in this cloud folder.",
};

/** Where the denied line sends the lawyer when the desktop gave no path. */
const SETTINGS = "System Settings › Privacy & Security";

export type Asked = {
  id: FolderPermissionId;
  folder: FolderRefId;
  /** Allow not pressed yet, macOS's dialog up, or turned down (with where to change it). */
  state: "ask" | "asking" | { denied: string };
};

/** The guarded folder's line for the latest pick: `picked` hears every pick, `clear` drops it. */
export function useFolderPermission(matters: Matters) {
  const [asked, setAsked] = useState<Asked | null>(null);
  // the latest pick's ref: an answer for an earlier pick is dropped
  const latest = useRef<FolderRefId | null>(null);
  const picked = (pick: Pick<PickedFolder, "folder" | "permission">) => {
    latest.current = pick.folder;
    const id = folderPermissionOf(pick);
    // no line where this window cannot ask: Allow would do nothing
    setAsked(
      id === null || matters.askFolderPermission === null
        ? null
        : { id, folder: pick.folder, state: "ask" },
    );
  };
  const clear = () => {
    latest.current = null;
    setAsked(null);
  };
  /** Allow: macOS's dialog for the folder picked. Granted, the line goes; anything else, the denied line. */
  const allow = async () => {
    const ask = matters.askFolderPermission;
    if (asked === null || ask === null) return false;
    const { id, folder } = asked;
    setAsked({ id, folder, state: "asking" });
    let denied: string;
    try {
      const row = await ask(id, folder);
      if (latest.current !== folder) return false;
      if (row.state === "granted") {
        setAsked(null);
        return true;
      }
      denied = row.settings_path ?? SETTINGS;
    } catch {
      if (latest.current !== folder) return false;
      denied = SETTINGS;
    }
    setAsked({ id, folder, state: { denied } });
    return false;
  };
  return { asked, picked, clear, allow };
}
