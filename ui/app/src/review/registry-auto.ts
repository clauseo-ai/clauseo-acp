// A new copy-review entries file goes in review/auto/ and default-exports Entry[]; never edit registry.tsx for it.
// This module collects them: every review/auto/*.tsx but its tests, in path order, so the order (and the page's)
// stays the same from build to build. registry.tsx spreads AUTO_ENTRIES once, at the end of its list, so the copy
// gate's checks (copy-gate.check.ts: duplicate and malformed ids, approvals) cover these entries too. A helper an
// entries file needs lives beside it as a .ts file, or outside auto/: each .tsx here must default-export an array.
// Files here import nothing from registry.tsx (it imports this module, so that would be a cycle).
import type { Entry } from "./entry.ts";

/** The entries of a glob's modules, file by file in path order; throws naming a file whose default export is not
 * an array. */
export function collectEntries(files: Readonly<Record<string, unknown>>): Entry[] {
  const entries: Entry[] = [];
  for (const path of Object.keys(files).sort()) {
    const list = (files[path] as { default?: unknown } | undefined)?.default;
    if (!Array.isArray(list))
      throw new Error(
        `copy review: ${path} must default-export its entries, an Entry[] (export default ENTRIES)`,
      );
    entries.push(...(list as Entry[]));
  }
  return entries;
}

const FILES = import.meta.glob(["./auto/*.tsx", "!./auto/*.test.tsx"], { eager: true });

/** The files collected, in path order: for the test that proves the glob finds them. */
export const AUTO_FILES: readonly string[] = Object.keys(FILES).sort();

export const AUTO_ENTRIES: readonly Entry[] = collectEntries(FILES);
