// The Hindi of words-sources.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-sources.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  // the folded line: how many distinct pages its searches listed and it read, the singular apart
  "1 source": "1 स्रोत",
  "{n} sources": "{n} स्रोत",
  // the folded line's name when its searches list no page, so it has no count
  Searches: "खोज",
  // a search: its query, quoted, after the search glyph; its results nested under it
  "'{query}'": "'{query}'",
  "No results": "कोई परिणाम नहीं",
  // Codex's search: its pages aren't told, after the info glyph
  "{name} doesn't list its pages": "{name} अपने पेज नहीं बताता",
  // a page Codex opened: its info tip's name and text
  "Opened on OpenAI's side": "OpenAI की ओर खोला गया",
  // a page the browser opened: the browser glyph's name and tooltip, in the tile's place
  "Opened in the browser": "ब्राउज़र में खोला गया",
} as const satisfies Hindi<string>;
