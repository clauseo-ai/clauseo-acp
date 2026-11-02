// Which permission prompt takes the keys: the oldest pending request an answer can still reach, found once per change
// of the id list or of a permission entry, so a flush that only appends text finds the same one without a scan. Apart
// from permission-model.ts, so the room's first load (Work, which provides it) carries only this.
import { createContext } from "react";
import type { StreamView } from "../store/view.ts";
import type { EntryId, PermissionRequestId } from "../transport/protocol.ts";

const PERMISSION = "permission:";
const permissionIds = new WeakMap<readonly EntryId[], readonly EntryId[]>();

/** The request whose prompt takes the keys: the oldest pending one an answer can still reach, or null. The view
 * replaces its id list on every add or remove and never changes one in place, so the permission ids are kept per id
 * list; a flush reads only the permission entries. */
export function keyedRequest(view: StreamView | undefined): PermissionRequestId | null {
  if (view === undefined) return null;
  let ids = permissionIds.get(view.ids);
  if (ids === undefined) {
    ids = view.ids.filter((id) => id.startsWith(PERMISSION));
    permissionIds.set(view.ids, ids);
  }
  for (const id of ids) {
    const p = view.entries.get(id)?.permission;
    if (p?.state === "pending" && p.deliverable === "live") return p.request;
  }
  return null;
}

/** The request that takes the keys in the open thread (Work provides it, from `keyedRequest`). */
export const KeyedRequestContext = createContext<PermissionRequestId | null>(null);
