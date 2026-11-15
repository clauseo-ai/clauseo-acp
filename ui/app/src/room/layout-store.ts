// Whether the room's sidebar (the rail) and papers (the wall) are shown, as the lawyer last left them, kept per window
// in localStorage under `clauseo.layout.<window label>` (`clauseo.layout.main` for the room's window), read through
// the same guard as update-seen.ts: with storage off it holds for this run only. A missing or unreadable value is
// both shown. UiSettings has no field for it: it is the window's, not the app's.
export type Layout = {
  /** The rail, beside the work from 1200 px. */
  sidebar: boolean;
  /** The wall of papers. */
  papers: boolean;
};

/** Both shown: a first run, or a value that can't be read. */
export const SHOWN: Layout = { sidebar: true, papers: true };

const KEY = "clauseo.layout.";

/** This run's own record per window, where storage is off. */
const run = new Map<string, Layout>();

/** localStorage, or null where reaching it throws (storage turned off). */
function pageStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

/** `raw` read as a layout; SHOWN for anything else: missing, not JSON, or not two flags. */
function parse(raw: string | null): Layout {
  if (raw === null) return SHOWN;
  try {
    const value: unknown = JSON.parse(raw);
    if (typeof value !== "object" || value === null) return SHOWN;
    const { sidebar, papers } = value as Record<string, unknown>;
    return typeof sidebar === "boolean" && typeof papers === "boolean"
      ? { sidebar, papers }
      : SHOWN;
  } catch {
    return SHOWN;
  }
}

/** The layout the window `label` was last left in. */
export function readLayout(label: string): Layout {
  const storage = pageStorage();
  if (storage === null) return run.get(label) ?? SHOWN;
  try {
    return parse(storage.getItem(KEY + label));
  } catch {
    return run.get(label) ?? SHOWN;
  }
}

/** Keeps `layout` as the window `label`'s. */
export function writeLayout(label: string, layout: Layout): void {
  run.set(label, layout);
  try {
    pageStorage()?.setItem(
      KEY + label,
      JSON.stringify({ sidebar: layout.sidebar, papers: layout.papers }),
    );
  } catch {
    // storage full or off: this run's record holds it
  }
}
