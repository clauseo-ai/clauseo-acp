// Named character references without the full HTML table on first load. micromark decodes them through
// decode-named-character-reference, which ui/patches/ replaces (ui/pnpm-workspace.yaml, patchedDependencies)
// for every consumer: about 150 common names (`&amp;`, `&copy;`, `&sect;`, `&mdash;`…) are known at once,
// and the full table (character-entities) is its own chunk, fetched here when the browser is idle after
// the first render. A block parsed before then that met a name the common set does not know is parsed
// again when the table arrives (answer.ts), and only blocks holding `&` re-render.
import {
  characterEntitiesLoaded,
  characterReferenceMisses,
  loadCharacterEntities,
  onCharacterEntities,
} from "decode-named-character-reference";

export { characterEntitiesLoaded, characterReferenceMisses, onCharacterEntities };

/** How long a browser without requestIdleCallback (WebKit) waits before fetching the table. */
const IDLE_FALLBACK_MS = 500;

let scheduled = false;

/** Fetch the full table once, when the browser is next idle; a failed fetch is tried again next time. */
export function prefetchEntities(): void {
  if (scheduled || characterEntitiesLoaded()) return;
  scheduled = true;
  const load = () => {
    loadCharacterEntities().catch(() => {
      scheduled = false;
    });
  };
  if (typeof requestIdleCallback === "function") requestIdleCallback(load, { timeout: 2000 });
  else setTimeout(load, IDLE_FALLBACK_MS);
}

/** Load the full table now (tests and the spec suites, which need every name). */
export function loadEntities(): Promise<void> {
  return loadCharacterEntities();
}
