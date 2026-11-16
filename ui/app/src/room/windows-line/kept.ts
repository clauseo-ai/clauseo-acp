// When the lawyer put away the Windows line's notice (UC-820, "Got it"): once is for good. Kept in localStorage under
// `clauseo.windows-line-seen`, as the time it was put away (an ISO date-time), read through the same guard as
// sac/kept.ts; with storage off it holds for this run only. UiSettings has no field for it.

const KEY = "clauseo.windows-line-seen";

/** This run's own record, where storage is off. */
const run: { at: string | null } = { at: null };

/** localStorage, or null where reaching it throws (storage turned off). */
function pageStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

/** When the notice was put away on this computer; null while it hasn't been. */
export function windowsLineSeen(): string | null {
  if (run.at !== null) return run.at;
  try {
    return pageStorage()?.getItem(KEY) ?? null;
  } catch {
    return null;
  }
}

/** Puts the notice away for good, recording when. */
export function markWindowsLineSeen(at: Date = new Date()): void {
  run.at = at.toISOString();
  try {
    pageStorage()?.setItem(KEY, run.at);
  } catch {
    // storage refused: this run's record holds it
  }
}

/** Forgets this run's record, for tests that remount with storage alone. */
export function forgetWindowsLineSeenForTests(): void {
  run.at = null;
}
