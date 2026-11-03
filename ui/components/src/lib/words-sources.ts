// The words of the web sources at the end of an answer: the one quiet line that sums them up, and each row once it is
// open (app/src/room/thread/sources.tsx). As in words.ts the English is the key, the exact string on screen. A row's
// {query} and {page} are filled by the row itself, a link or an isolated run, so the row splits its string at them
// (partsOf); {name} is the plan's name, "Claude" or "ChatGPT".
// The Hindi waits for #285 (null Hindi): every value here is its English, so each swaps to null by hand.
import { useDisplay } from "./display.ts";
import type { Lang } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";

// hi: later (every value below is its English text until #285 lands)
const HI = {
  // the summary line, one sentence for the searches and one for the pages, the singular apart
  "1 web search.": "1 web search.",
  "{n} web searches.": "{n} web searches.",
  "1 page read.": "1 page read.",
  "{n} pages read.": "{n} pages read.",
  // a search, its results each on a line of their own under it
  "Searched the web for {query}.": "Searched the web for {query}.",
  "No results.": "No results.",
  "{name} doesn't report which pages it found.": "{name} doesn't report which pages it found.",
  // a page, by how it was opened
  "Read {page}.": "Read {page}.",
  "{name} opened this page on OpenAI's computers, not on this one.":
    "{name} opened this page on OpenAI's computers, not on this one.",
  "Opened {page} in the browser.": "Opened {page} in the browser.",
} as const;

/** Every string of the web sources, in English. */
export type SourcesKey = keyof typeof HI;

/** A string of the web sources in the language, its slots filled. */
export const src = wordsOf<SourcesKey>(HI);

/** The summary line: "2 web searches. 3 pages read.", either sentence alone when the other has none; empty for
 * none of either. */
export function sourcesSummary(lang: Lang, searches: number, pages: number): string {
  const said: string[] = [];
  if (searches > 0)
    said.push(
      searches === 1 ? src(lang, "1 web search.") : src(lang, "{n} web searches.", { n: searches }),
    );
  if (pages > 0)
    said.push(pages === 1 ? src(lang, "1 page read.") : src(lang, "{n} pages read.", { n: pages }));
  return said.join(" ");
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

/** The web sources' words of the page's language, bound: const w = useSourcesWords(); w.src("No results."). */
export function useSourcesWords() {
  const { lang } = useDisplay();
  return {
    lang,
    src: (key: SourcesKey, slots?: Slots) => src(lang, key, slots),
    parts: (key: SourcesKey, slot: string, slots?: Slots) => partsOf(lang, key, slot, slots),
    summary: (searches: number, pages: number) => sourcesSummary(lang, searches, pages),
  };
}
