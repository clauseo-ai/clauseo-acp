// What the host found about a matter's folder (MatterSummary.folder_support): where Clauseo can't fence Claude and
// Codex inside it. Absent, the folder is fine or not checked since the host started; "unknown", a newer Clauseo's
// reading, changes nothing here. Kept apart from the notice (folder-notice.tsx) and its words, which load later.
import { createContext, use } from "react";
import type { FolderSupportKind } from "../transport/protocol.ts";

/** The readings this version shows: the room's notice, and Claude asking before every change. */
export type FolderNoted = Exclude<FolderSupportKind, "unknown" | "check_failed">;

/** The reading this version knows, or null: fine, not checked, or a newer Clauseo's. */
export const notedFolder = (kind: FolderSupportKind | undefined): FolderNoted | null =>
  kind === "network" || kind === "no_permissions" || kind === "cant_change_permissions"
    ? kind
    : null;

/** A folder Clauseo can't fence at all, so the matter is Not protected: a network drive, or one without permissions. */
export const unprotectedFolder = (kind: FolderSupportKind | undefined) =>
  kind === "network" || kind === "no_permissions";

/** The open matter's folder reading, for the thread's permission cards (folder-reason.tsx). */
export const FolderSupportContext = createContext<FolderSupportKind | undefined>(undefined);

/** Whether a pending card for a call of this kind gives the folder's reason: a file edit, in such a matter. */
export function useFolderReason(kind: string | undefined): boolean {
  return use(FolderSupportContext) === "cant_change_permissions" && kind === "edit";
}
