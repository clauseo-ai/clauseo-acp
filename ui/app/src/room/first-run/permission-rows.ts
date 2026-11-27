// The first screen's permissions step's rows as it draws them (issue 5), apart from permissions.tsx, which exports
// only its component: the host's rows in order, and what each shows; and which rows are not asked yet.
import type { PermissionStepRow } from "@clauseo/components/clauseo/first-launch/permissions.tsx";
import type { PermissionId, PermissionRow } from "./permissions-host.ts";

type Listed = PermissionStepRow["id"];

/** The Mac's order: the Documents folder, then the keychain; Windows' one row after. */
const ORDER: readonly Listed[] = ["files_documents", "keychain", "controlled_folder_access"];

/** A row `permissions` lists; the pick-time ids (files_desktop and the rest) never are. */
const listed = (row: PermissionRow): row is PermissionRow & { id: Listed } =>
  (ORDER as readonly PermissionId[]).includes(row.id);

/** Not asked yet: a Mac row before its ask, and a keychain row whose silent check couldn't tell ("unknown"), which an
 * Allow settles. Windows' "unknown" is a block that can't be read, not a question. */
export const unasked = (row: PermissionRow) =>
  row.state === "not_asked" ||
  row.state === "would_ask" ||
  (row.id === "keychain" && row.state === "unknown");

/** macOS's dialog open for the row: the desktop says so ("asking"), or this window's ask (`waiting`, permissions-host.ts's
 * pending) is under way and no read since has said otherwise. Whatever a later read says (granted, or denied, which a
 * crashed or timed-out ask gives too) ends the wait. The one place that decides it, for the step's rows and every
 * keychain pointer. */
export const isAsking = (row: PermissionRow, waiting: PermissionId | null) =>
  row.state === "asking" || row.id === waiting;

/** The row still wants the lawyer's Allow: not asked yet (unasked), macOS asking, or a refused keychain row (its saved
 * keys read as unreadable until it's allowed). The one rule for the band's standing and every keychain pointer. */
export const awaitsAllow = (row: PermissionRow) =>
  unasked(row) || row.state === "asking" || (row.id === "keychain" && row.state === "denied");

/** The Documents row listed not asked yet on a Mac that answered it before (documents-answered.ts): the desktop forgets
 * its answers at each update (store.rs keys them by build), while macOS keeps the lawyer's. The band gives it a quiet
 * line, not the full step. */
export const quietDocuments = (row: PermissionRow, answered: boolean) =>
  answered && row.id === "files_documents" && unasked(row);

/** The keychain row while it awaits the lawyer's Allow (awaitsAllow); granted, or no row (Windows, no host): null. */
export const keychainRow = (rows: readonly PermissionRow[] | null): PermissionRow | null =>
  rows?.find((r) => r.id === "keychain" && awaitsAllow(r)) ?? null;

/** The rows the step draws, in order; every row not asked yet has its own Allow, whatever the other rows' state (the
 * keychain's never waits on Documents' answer). */
export function stepRows(
  rows: readonly PermissionRow[],
  waiting: PermissionId | null,
  failed: PermissionId | null,
  /** The keychain was allowed once only (permissions-host.ts's allowedOnce): its row says so, with its Allow. */
  once = false,
): PermissionStepRow[] {
  const sorted = rows.filter(listed).sort((a, b) => ORDER.indexOf(a.id) - ORDER.indexOf(b.id));
  return sorted.map((row) => {
    const given = {
      ...(row.folder === null ? {} : { folder: row.folder }),
      ...(row.settings_path === null ? {} : { path: row.settings_path }),
      ...(row.note === null ? {} : { note: row.note }),
    };
    // macOS's dialog open, asked from here or before this window read (#1344): waiting, no second Allow
    if (isAsking(row, waiting)) return { id: row.id, shows: "waiting", ...given };
    if (unasked(row)) {
      const shows = row.id === failed ? "failed" : once && row.id === "keychain" ? "once" : "ask";
      return { id: row.id, shows, ...given };
    }
    // a refused keychain row keeps its Allow (it can be allowed at any time); its failed ask says so
    if (row.id === failed && row.id === "keychain")
      return { id: row.id, shows: "failed", ...given };
    const shows =
      row.state === "granted" || row.state === "blocking" || row.state === "unknown"
        ? row.state
        : "denied";
    return { id: row.id, shows, ...given };
  });
}
