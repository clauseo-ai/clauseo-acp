// R23: "Clauseo was updated from 0.4.0 to 0.4.1" shows once per update. Got it keeps the version it was pressed on in
// localStorage under `clauseo.update-seen`, read through the same guard as protection/explained.ts; with storage off
// it stays dismissed for this run only. UiSettings has no field for it.
const KEY = "clauseo.update-seen";

/** This run's own record, where storage is off. */
let run: string | null = null;

/** localStorage, or null where reaching it throws (storage turned off). */
function pageStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

/** The version whose update note was last dismissed; null for none. */
export function seenUpdate(): string | null {
  const storage = pageStorage();
  if (storage === null) return run;
  try {
    return storage.getItem(KEY);
  } catch {
    return run;
  }
}

/** Marks the update to `version` seen, so its note doesn't show again. */
export function markUpdateSeen(version: string): void {
  run = version;
  try {
    pageStorage()?.setItem(KEY, version);
  } catch {
    // storage full or off: this run's record holds it
  }
}
