// The `@` menu's rules, apart from the DOM: what a picked mention reads as in the field, and which of the mentionable
// things a query matches, in what order. It loads with the menu's chunk. The list is folded once, as it arrives, so a
// keystroke folds only the query: the 2,000 papers H8 lists at most filter in well under the 8 ms frame budget
// (composer-mention.bench.ts).
import type { MentionItem } from "./composer.tsx";

/** At most this many rows stand in the menu; a line under them says how many matched. */
export const MAX_MENTION_ROWS = 50;

// whitespace, a bracket or a quote would end or confuse the token as plain words: such a path goes in quotes
const QUOTE = /[\s()[\]{}<>"]/u;

/** What a picked mention puts in the field: `@` and its text (a paper's path in the matter folder), in double quotes
 * when the text holds whitespace, a bracket or a quote: `@Notice.pdf`, `@"Pleadings/Board minutes.pdf"`. The token is
 * held by its range (composer-tokens.ts), never parsed back, so a quote inside the quotes needs no escape. */
export function mentionText(text: string): string {
  return QUOTE.test(text) ? `@"${text}"` : `@${text}`;
}

/** The text compared: NFC, so a Devanagari or Tamil name typed with the composed or the decomposed sequence matches
 * the other, and lower case. */
const fold = (s: string) => s.normalize("NFC").toLowerCase();

/** A mentionable thing with its name and its text folded, once, as the list arrives. */
export type Folded = { item: MentionItem; name: string; text: string };

export function foldMentions(items: readonly MentionItem[]): Folded[] {
  return items.map((item) => ({ item, name: fold(item.label), text: fold(item.text) }));
}

/** The things the query matches, at most `max`, and how many matched in all. A match is a substring of the name, or
 * else of the text (the path, its folders included), case and normalisation aside; names that start with the query
 * come first, then other name matches, then path matches, each in the list's own order. A leading `"`, as the quoted
 * form starts, is no part of the query. */
export function filterMentions(
  list: readonly Folded[],
  query: string,
  max: number = MAX_MENTION_ROWS,
): { items: MentionItem[]; total: number } {
  const q = fold(query.startsWith('"') ? query.slice(1) : query);
  if (q === "") return { items: list.slice(0, max).map((f) => f.item), total: list.length };
  const prefix: MentionItem[] = [];
  const inName: MentionItem[] = [];
  const inText: MentionItem[] = [];
  let total = 0;
  for (const f of list) {
    const i = f.name.indexOf(q);
    const group = i === 0 ? prefix : i > 0 ? inName : f.text.includes(q) ? inText : null;
    if (group === null) continue;
    total += 1;
    if (group.length < max) group.push(f.item);
  }
  return { items: [...prefix, ...inName, ...inText].slice(0, max), total };
}

/** The row to highlight in a new list: the one with the highlighted row's id, or the first. */
export function keepMention(items: readonly MentionItem[], id: string | null): number {
  if (id === null) return 0;
  const i = items.findIndex((m) => m.id === id);
  return i === -1 ? 0 : i;
}

/** The mentions a send carries: each token's thing, in the order the tokens first stand in the field, each once. */
export function mentionedIn(
  tokens: readonly { text: string }[],
  known: ReadonlyMap<string, MentionItem>,
): MentionItem[] {
  const seen = new Set<string>();
  const out: MentionItem[] = [];
  for (const t of tokens) {
    const m = known.get(t.text);
    if (m === undefined || seen.has(m.id)) continue;
    seen.add(m.id);
    out.push(m);
  }
  return out;
}
