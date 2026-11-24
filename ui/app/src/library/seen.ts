// Whether the lawyer has opened Skills and connectors: kept in this computer's localStorage, as the
// display settings are, so the rail's "New" tag shows until the first visit.

/** The localStorage key set once the place has been opened. */
export const SEEN_KEY = "clauseo.library-seen";

export function wasSeen(): boolean {
  try {
    return globalThis.localStorage?.getItem(SEEN_KEY) === "1";
  } catch {
    // no storage (a sandboxed page): the tag shows for this run
    return false;
  }
}

export function markSeen(): void {
  try {
    globalThis.localStorage?.setItem(SEEN_KEY, "1");
  } catch {
    // only the tag reads it
  }
}
