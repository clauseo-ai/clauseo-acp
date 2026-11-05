// The Hindi of words-sources.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-sources.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  // the folded line: how many distinct pages its searches listed and it read, the singular apart
  "1 source": null,
  "{n} sources": null,
  // the folded line's name when its searches list no page, so it has no count
  Searches: null,
  // a search: its query, quoted, after the search glyph; its results nested under it
  "'{query}'": null,
  "No results": null,
  // Codex's search: its pages aren't told, after the info glyph
  "{name} doesn't list its pages": null,
  // a page Codex opened: its info tip's name and text
  "Opened on OpenAI's side": null,
  // a page the browser opened: the browser glyph's name and tooltip, in the tile's place
  "Opened in the browser": null,
} as const satisfies Hindi<string>;
