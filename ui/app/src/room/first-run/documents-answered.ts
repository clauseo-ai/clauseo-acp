// RC A4: whether this Mac has answered the Documents row before (granted or denied, on any build). The desktop keeps
// its answers per build (crates/desktop/src/permissions/store.rs: a store naming another build reads as nothing asked),
// so after every update it lists Documents as not_asked again, though macOS still holds the lawyer's answer. Kept in
// localStorage under `clauseo.documents-answered`, read through the same guard as update-seen.ts; with storage off it
// holds for this run only. UiSettings has no field for it.
const KEY = "clauseo.documents-answered";

/** This run's own record, where storage is off. */
let run = false;

/** localStorage, or null where reaching it throws (storage turned off). */
function pageStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

/** Whether this Mac answered Documents at an earlier read. */
export function documentsAnswered(): boolean {
  const storage = pageStorage();
  if (storage === null) return run;
  try {
    return storage.getItem(KEY) === "1" || run;
  } catch {
    return run;
  }
}

/** Marks Documents answered on this Mac. */
export function markDocumentsAnswered(): void {
  if (run) return;
  run = true;
  try {
    pageStorage()?.setItem(KEY, "1");
  } catch {
    // storage full or off: this run's record holds it
  }
}

/** A test's: forgets this run's record (the storage is the test's to clear). */
export function forgetDocumentsAnswered(): void {
  run = false;
}
