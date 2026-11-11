// The cloud services a folder may be synced by, by their owners' names: for New matter's scan (scan/use-scan.ts) and
// for the wall's papers that are only online (wall-away.ts). Small and alone, so neither chunk carries the other.
import type { FolderPlace, SyncService } from "../transport/protocol.ts";

/** Each service by its owner's name; "other" has none, so no synced notice, and cloud-only files are kept "online".
 * A service this version doesn't know reads as other. */
const SERVICES: Partial<Record<SyncService, string>> = {
  one_drive: "OneDrive",
  google_drive: "Google Drive",
  icloud: "iCloud Drive",
  dropbox: "Dropbox",
  box: "Box",
};

/** The name of the service that syncs a folder at `place`; null where none with a name to show does. */
export function serviceName(place: FolderPlace | undefined): string | null {
  if (place === undefined || typeof place !== "object" || place.synced === undefined) return null;
  return SERVICES[place.synced.service] ?? null;
}
