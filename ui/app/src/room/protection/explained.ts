// UC: "explained once" (Decided here). The Not protected note opens by itself the first time a room opens on a matter
// in that state: once per computer for set-up and can't, once per matter for switched off, never for a full pool
// (it's passing, and the mark opens it). Kept in localStorage under `clauseo.protection-explained`, read through the
// same guard as display/connect.ts; with storage off the note shows once each run. UiSettings has no field for it.
import type { MatterId } from "../../transport/protocol.ts";
import type { CauseGroup } from "./causes.ts";

const KEY = "clauseo.protection-explained";

type Explained = { computer: boolean; matters: MatterId[] };

/** This run's own record, where storage is off. */
const run: Explained = { computer: false, matters: [] };

/** localStorage, or null where reaching it throws (storage turned off). */
function pageStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

function read(): Explained {
  const storage = pageStorage();
  if (storage === null) return run;
  try {
    const kept = JSON.parse(storage.getItem(KEY) ?? "null") as Partial<Explained> | null;
    return {
      computer: kept?.computer === true,
      matters: Array.isArray(kept?.matters)
        ? kept.matters.filter((m) => typeof m === "string")
        : [],
    };
  } catch {
    return { computer: false, matters: [] };
  }
}

/** Whether the note should open by itself for this group and matter: not explained yet. A read only. */
export function explainDue(group: CauseGroup, matter: MatterId): boolean {
  if (group === "pool") return false;
  const now = read();
  return group === "off" ? !now.matters.includes(matter) : !now.computer;
}

/** Marks the group (the computer's, or the matter's when switched off) explained. */
export function markExplained(group: CauseGroup, matter: MatterId): void {
  if (group === "pool") return;
  const now = read();
  const next: Explained =
    group === "off"
      ? { ...now, matters: now.matters.includes(matter) ? now.matters : [...now.matters, matter] }
      : { ...now, computer: true };
  const storage = pageStorage();
  if (storage === null) Object.assign(run, next);
  else
    try {
      storage.setItem(KEY, JSON.stringify(next));
    } catch {
      Object.assign(run, next);
    }
}
