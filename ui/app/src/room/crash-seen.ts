// The notice after Clauseo closed unexpectedly shows once per unclean exit. Got it keeps the start time of the run it
// was pressed in (LastExit's run_started, the same all through a run) in localStorage under `clauseo.crash-seen`, so a
// reload of the window doesn't bring it back; the next unclean exit has a new run, and shows again. Read through the
// same guard as update-seen.ts; with storage off it stays dismissed for this page only.
const KEY = "clauseo.crash-seen";

/** This page's own record, where storage is off. */
let run: string | null = null;

/** localStorage, or null where reaching it throws (storage turned off). */
function pageStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

/** The run whose notice was last dismissed; null for none. */
export function seenCrash(): string | null {
  const storage = pageStorage();
  if (storage === null) return run;
  try {
    return storage.getItem(KEY);
  } catch {
    return run;
  }
}

/** Marks the notice of the run that started at `started` seen. */
export function markCrashSeen(started: string): void {
  run = started;
  try {
    pageStorage()?.setItem(KEY, started);
  } catch {
    // storage full or off: this page's record holds it
  }
}
