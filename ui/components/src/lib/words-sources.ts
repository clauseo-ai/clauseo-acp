// The words of the web sources at the end of an answer (app/src/room/thread/sources.tsx), the fewest that still read:
// the rows lead with icons (a site's tile, the search glyph, the browser glyph), so the words are the count when it is
// folded, a search's query in quotes, and a few muted words where an icon can't say it. As in words.ts the English is
// the key, the exact string on screen. A search's {query} is filled by an isolated run, so the row splits its string at
// it (partsOf); {name} is the plan's name, "Claude" or "ChatGPT".
// Every string ships English only: its Hindi is null (hi: later), so the Hindi interface shows the English.
import { useDisplay } from "./display.ts";
import type { Lang } from "./display.ts";
import { wordsOf, type Hindi, type Slots } from "./words-core.ts";

const HI = {
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

/** Every string of the web sources, in English. */
export type SourcesKey = keyof typeof HI;

/** A string of the web sources in the language, its slots filled. */
export const src = wordsOf<SourcesKey>(HI);

/** The summary line: how many distinct pages, "3 sources" or "1 source". */
export function sourcesSummary(lang: Lang, rows: number): string {
  return rows === 1 ? src(lang, "1 source") : src(lang, "{n} sources", { n: rows });
}

/** A string split at its one `{slot}`: the words before it and after it, its other slots filled. A string without the
 * slot is all before. */
export function partsOf(
  lang: Lang,
  key: SourcesKey,
  slot: string,
  slots?: Slots,
): [before: string, after: string] {
  const text = src(lang, key, slots);
  const at = text.indexOf(`{${slot}}`);
  return at < 0 ? [text, ""] : [text.slice(0, at), text.slice(at + slot.length + 2)];
}

/** The web sources' words of the page's language, bound: const w = useSourcesWords(); w.src("No results"). */
export function useSourcesWords() {
  const { lang } = useDisplay();
  return {
    lang,
    src: (key: SourcesKey, slots?: Slots) => src(lang, key, slots),
    parts: (key: SourcesKey, slot: string, slots?: Slots) => partsOf(lang, key, slot, slots),
    summary: (rows: number) => sourcesSummary(lang, rows),
  };
}
