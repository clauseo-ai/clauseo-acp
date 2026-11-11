// Whether the lawyer has put away the Smart App Control note ("Got it"): once is for good. Kept in localStorage under
// `clauseo.smart-app-control-noted`, read through the same guard as protection/explained.ts; with storage off it holds
// for this run only. UiSettings has no field for it.

const KEY = "clauseo.smart-app-control-noted";

/** This run's own record, where storage is off. */
const run = { noted: false };

/** localStorage, or null where reaching it throws (storage turned off). */
function pageStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

/** Whether "Got it" was pressed on this computer before. */
export function sacNoted(): boolean {
  if (run.noted) return true;
  try {
    return pageStorage()?.getItem(KEY) === "true";
  } catch {
    return false;
  }
}

/** Puts the note away for good. */
export function markSacNoted(): void {
  run.noted = true;
  try {
    pageStorage()?.setItem(KEY, "true");
  } catch {
    // storage refused: this run's record holds it
  }
}

/** Forgets this run's record, for tests that remount with storage alone. */
export function forgetSacNotedForTests(): void {
  run.noted = false;
}
