// What the append fast path's tests share (append.test.ts, append-fixtures.test.ts): a seeded random source,
// stream generators aimed at the edges of what the fast path takes, and the check that two updates' views are
// the same: their fields, their React elements and (against a fresh render) the hast they make again.
import { expect } from "vitest";
import { createAnswer } from "./answer.ts";
import type { BlockView } from "./answer.ts";
import { appendProbe } from "./append.ts";
import { parseProbe } from "./parse.ts";

/** mulberry32: a seeded source of numbers in [0, 1). */
export function randomOf(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296;
  };
}

const pick = <T>(random: () => number, items: readonly T[]): T =>
  items[Math.floor(random() * items.length)]!;

/** Allowlisted words: English, Devanagari and Tamil runs, digits, every allowlisted punctuation mark. */
export const PLAIN_WORDS = [
  "the",
  "Supplier",
  "shall",
  "notify",
  "Client",
  "within",
  "days",
  "clause",
  "8.3",
  "2024",
  "agreement",
  "अनुबंध",
  "की",
  "शर्तें",
  "लागू",
  "होंगी।",
  "ஒப்பந்தம்",
  "விதிகள்",
  "பொருந்தும்.",
  "(a)",
  '"quoted"',
  "it's",
  "and/or",
  "Note:",
  "really?",
  "yes!",
  "semi;",
  "x-ray",
  "e.g.,",
  "ww.",
  "http:",
  "www",
  "wwx.",
  "mailto",
  "xmpp",
  "WW.",
  "https",
  "𝐀𝐁",
  "naïve",
  '")',
  "')",
  ")",
];

/**
 * Inline syntax the fast path's last line may hold before the append point (closed and unclosed): emphasis
 * of each kind, a tag with no `>`, a reference and a footnote call (defined or not), an unclosed backtick,
 * trailing spaces, and what must make it fall back: an autolink-like run one character short and whole, a
 * character reference, an email, an escape, an unsettled link.
 */
export const MARKUP = [
  "*a* ",
  "**bold** ",
  "*open ",
  "**open ",
  "_u_ ",
  "_open ",
  "~~s~~ ",
  "~~open ",
  "<b ",
  "<tag",
  '<sup class="x ',
  "<b>x</b> ",
  "[x] ",
  "[^1] ",
  "[x]",
  "[^2] ",
  "`open ",
  "`code` ",
  "  ",
  "&amp; ",
  "&amp",
  "a@b.co ",
  "\\",
  "\\* ",
  "[a](https://example.com/a) ",
  '[a](b "t',
  "^2^ ",
  "==hi== ",
  "==open ",
  "www.example.com ",
  "https://example.com/x ",
  "!",
  "| ",
  "# ",
  "]( ",
  "xmpp:",
  "mailto:",
  "http:/",
  "ww.w",
  "ww",
  "<",
  "]",
  "[x ",
  "[y ",
  '[a [b](c "t](d) ',
  "![a [b](c 't](d) ",
];

/**
 * Block starts and ends: line endings, lists, headings, quotes, tables, definitions, notes, fences, and lines
 * starting with a short emphasis run before a letter (taken) or with what starts a block (not taken).
 */
export const BREAKS = [
  "\n",
  "\n\n",
  "\n\n- item ",
  "\n\n1. first ",
  "\n\n# Heading\n\n",
  "\n\n> quote ",
  "\n\n| a | b |\n| - | - |\n| 1 | 2 |\n\n",
  "\n\n[x]: https://example.com/x\n\n",
  "\n\n[^1]: The note.\n\n",
  "\n\n```\ncode\n```\n\n",
  "\nTerm\n: definition\n\n",
  "\n   indented ",
  "\n---\n",
  "\n===\n",
  "\n- ",
  "\n[y]:\n",
  "\n: ",
  "\n*x ",
  "\n**x ",
  "\n***x ",
  "\n_x ",
  "\n~~x ",
  "\n~~~x ",
  "\n* x ",
  "\n***\n",
  "\n***",
  "\n\n**Clause 8.3** obliges ",
  "\n    four spaces ",
  '\n\n[x]: https://example.com/x\n"multi\n',
  "\n\n[x]: https://example.com/x\n(multi ",
  "\n\n[x]:\nhttps://example.com/x\n'multi ",
];

/**
 * What may stand before the paragraph an append extends: nothing, a line of it, a definition (with a blank
 * line after it, or a title still arriving on the next lines), a note, a heading, a definition list.
 */
const CONTEXTS = [
  "",
  "A first line\n",
  "[x]: https://example.com/x\n\n",
  '[x]: https://example.com/x\n"multi\n',
  "[x]: https://example.com/x\n(multi\n",
  "[x]:\nhttps://example.com/x\n'multi\n",
  "[^1]: The note.\n\n",
  "# Heading\n\n",
  "Term\n: definition\n\n",
  "[y]: /y\n",
];

/** What an append may add: allowlisted words, and the syntax that would close or start something. */
export const DELTAS = [
  ...PLAIN_WORDS,
  "]",
  "](y)",
  "] (y)",
  "(y)",
  ")",
  '")',
  "')",
  '"',
  "'",
  "<",
  "<b>",
  ">",
  "*",
  "_",
  "~~",
  "`",
  "[",
  "&",
  ";",
  "@b.co",
  "\\",
  "\n",
  "\nx",
];

/** The syntax on the line an append extends that keeps the fast path open (nothing READS_ON in append.ts). */
const LINE_MARKUP = MARKUP.filter((markup) => !/[\\&@]|www\.|:\/|xmpp:|mailto:/i.test(markup));

/** A paragraph's source, maybe after a context, whose last line starts with a letter: what the fast path extends. */
export function lineText(random: () => number): string {
  let line = pick(random, ["Some", "**Clause 8.3**", "_Term_", "अनुबंध", "Note"]);
  for (let part = Math.floor(random() * 8); part > 0; part--)
    line += `${random() < 0.7 ? " " : ""}${pick(random, random() < 0.5 ? PLAIN_WORDS : LINE_MARKUP)}`;
  return pick(random, CONTEXTS) + (random() < 0.5 ? line.trimEnd() : line);
}

/** A random answer: mostly allowlisted words, with markup and block breaks between them. */
export function streamText(random: () => number, length: number): string {
  let text = "";
  while (text.length < length) {
    const roll = random();
    if (roll < 0.72) text += `${pick(random, PLAIN_WORDS)}${random() < 0.9 ? " " : ""}`;
    else if (roll < 0.92) text += pick(random, MARKUP);
    else text += pick(random, BREAKS);
  }
  return text;
}

/** Random delta sizes covering `length`, each from 1 to `most`. */
export function deltas(random: () => number, length: number, most: number): number[] {
  const sizes: number[] = [];
  for (let total = 0; total < length; total += sizes.at(-1)!)
    sizes.push(1 + Math.floor(random() * most));
  return sizes;
}

/** Random delta sizes covering `length`: mostly a few characters, sometimes a line's worth. */
export function deltasFor(random: () => number, length: number): number[] {
  const sizes: number[] = [];
  for (let total = 0; total < length;) {
    const size = random() < 0.85 ? 1 + Math.floor(random() * 12) : 13 + Math.floor(random() * 81);
    sizes.push(size);
    total += size;
  }
  return sizes;
}

/**
 * A view's hast made again; a note's without positions, which say where its note stood in the input its
 * block was parsed from: an answer that streamed parsed it with other placeholders before it than a fresh one
 * (so with the fast path off too).
 */
const placeless = (view: BlockView): unknown =>
  view.note === undefined
    ? view.hast
    : JSON.parse(
        JSON.stringify(view.hast, (key, value: unknown) =>
          key === "position" ? undefined : value,
        ),
      );

const FIELDS = [
  "source",
  "input",
  "streaming",
  "notes",
  "note",
  "slugs",
  "continues",
  "lostReferences",
] as const;

/**
 * Expects `got` and `want`, two updates' views, to be the same: their fields and their React elements, and
 * with `hast` the hast each makes again. A pair `same` already holds is not compared again.
 */
export function expectSame(
  got: readonly BlockView[],
  want: readonly BlockView[],
  label: string,
  hast: boolean,
  same?: WeakMap<BlockView, BlockView>,
): void {
  expect(got.length, label).toBe(want.length);
  for (const [index, view] of got.entries()) {
    const other = want[index]!;
    if (same?.get(view) === other) continue;
    const at = `${label} view ${index}`;
    for (const field of FIELDS)
      // A note's view's input is a cache key holding an id its answer numbers by identity (answer.ts).
      if (field !== "input" || !(view.notes || view.note))
        expect(view[field], `${at} ${field}`).toEqual(other[field]);
    expect(view.content, `${at} content`).toEqual(other.content);
    if (hast) expect(placeless(view), `${at} hast`).toEqual(placeless(other));
    if (view.items) expectSame(view.items, other.items ?? [], `${at} notes`, hast, same);
    same?.set(view, other);
  }
}

/** What a run of streams did: its updates, and those that took the fast path (at least one parse spared). */
export type Totals = { updates: number; fastUpdates: number; fast: number };

export const newTotals = (): Totals => ({ updates: 0, fastUpdates: 0, fast: 0 });

/** One update of `answer`, with the fast path's hits added to `totals`. */
function counted(answer: ReturnType<typeof createAnswer>, text: string, totals: Totals) {
  const counts = { parses: 0, chars: 0, fast: 0 };
  parseProbe.counts = counts;
  try {
    return answer.update(text, true);
  } finally {
    parseProbe.counts = undefined;
    totals.updates++;
    totals.fast += counts.fast;
    if (counts.fast > 0) totals.fastUpdates++;
  }
}

/**
 * Streams `text` in `deltas` into one answer, and expects every update, then the finished one, to equal a fresh
 * answer's single update of the same text, hast included.
 */
export function againstFresh(text: string, deltas: readonly number[], totals: Totals): void {
  const answer = createAnswer("p");
  let end = 0;
  for (const size of deltas) {
    end = Math.min(text.length, end + size);
    const prefix = text.slice(0, end);
    const views = counted(answer, prefix, totals);
    expectSame(views, createAnswer("p").update(prefix, true), JSON.stringify(prefix), true);
  }
  const once = createAnswer("p").update(text, false);
  expectSame(answer.update(text, false), once, `finished ${JSON.stringify(text)}`, true);
}

/**
 * Streams `text` in `deltas` into two answers, one with the fast path and one without, and expects every
 * update's last two views (the tail and the notes, all the fast path reads into) to be the same, fields and
 * elements, and as many views; every `freshEvery`-th update, and the finished one, all of them to equal a
 * fresh answer's. Not the hast they make again: that parses their input (compared) without the fast path.
 */
export function againstSlow(
  text: string,
  deltas: readonly number[],
  totals: Totals,
  freshEvery: number,
): void {
  const fast = createAnswer("p");
  const slow = createAnswer("p");
  const same = new WeakMap<BlockView, BlockView>();
  let end = 0;
  for (const [index, size] of deltas.entries()) {
    end = Math.min(text.length, end + size);
    const prefix = text.slice(0, end);
    appendProbe.off = true;
    const want = slow.update(prefix, true);
    appendProbe.off = false;
    const got = counted(fast, prefix, totals);
    const label = `at ${end}: ${JSON.stringify(prefix.slice(-80))}`;
    expect(got.length, label).toBe(want.length);
    expectSame(got.slice(-2), want.slice(-2), label, false, same);
    if (index % freshEvery === freshEvery - 1)
      expectSame(got, createAnswer("p").update(prefix, true), label, false, same);
  }
  const once = createAnswer("p").update(text, false);
  expectSame(fast.update(text, false), once, "finished", false);
}

/**
 * Streams `text` in `deltas` into one answer and expects every `freshEvery`-th update, and the finished one,
 * to equal a fresh answer's (fields and elements).
 */
export function againstSampled(
  text: string,
  deltas: readonly number[],
  totals: Totals,
  freshEvery: number,
): void {
  const answer = createAnswer("p");
  let end = 0;
  for (const [index, size] of deltas.entries()) {
    end = Math.min(text.length, end + size);
    const prefix = text.slice(0, end);
    const got = counted(answer, prefix, totals);
    if (index % freshEvery === freshEvery - 1)
      expectSame(got, createAnswer("p").update(prefix, true), `at ${end}`, false);
  }
  expectSame(answer.update(text, false), createAnswer("p").update(text, false), "finished", false);
}
