// Hindi clusters split at every UTF-16 offset across the cluster while an answer streams: the conjuncts (क्ष त्र
// ज्ञ श्र), a reph (र्म, र्क), a nukta (ज़ क़ फ़, decomposed and precomposed), a ZWJ and a ZWNJ form (क्‍ष, क्‌ष),
// and a conjunct with a vowel sign and an anusvara or chandrabindu (क्षें श्रीं श्रीँ). Each sits in a realistic
// sentence, in a paragraph, a heading, a list item and a table cell. Every cluster is cut into two chunks at each
// of its offsets (the start to the end, both included), and the ZWJ cluster also into three chunks at each pair of
// offsets. At every update the view must equal a fresh answer's (hast included), must show exactly the
// prefix's text once Markdown is stripped (no U+FFFD, nothing dropped, duplicated or reordered), and must end at
// the chunk's own end. The finished render must equal a one-shot render.
//
// What shows mid-stream, decided and pinned here: the view shows what has arrived and holds nothing back to a
// grapheme boundary. So the text can end inside a cluster: after a virama (`परीक्` before `ष` arrives), after a
// base before its nukta (`ज` before `़`), or after a joiner. Each such end is the chunk's end: a combining mark is
// shown only after its base (the prefix property), and nothing beyond the chunk. `midGrapheme` counts how often a
// view ended inside a grapheme of the finished text. That depends on the runtime's segmenter (Unicode 15.1's
// conjunct rule, GB9c, makes a conjunct one grapheme; older ICUs do not), so no count is pinned; only the decomposed
// nukta cases assert that some end is mid-grapheme, which holds on every ICU: a base before its nukta is no boundary.
//
// The #292 fast path (append.ts) takes only a paragraph's last text: a second update whose delta is plain
// (letters, marks, digits, space, punctuation) and whose last character is plain. A ZWJ or ZWNJ (Cf) is not
// plain, so a joiner cluster's inner cuts are full parses; the cut after the cluster is a fast one. A heading,
// list item or table cell never takes it. Each case's paragraph counts are pinned on that basis.
import { expect, test } from "vitest";
import { createAnswer } from "./answer.ts";
import type { BlockView } from "./answer.ts";
import { expectSame } from "./append-fixture.ts";
import { loadEntities } from "./entities.ts";
import { parse, parseProbe } from "./parse.ts";

await loadEntities();

const NUKTA = "\u093C";
const ZWJ = "\u200D";
const ZWNJ = "\u200C";
const PRECOMPOSED_NUKTA_JA = "\u095B"; // ज़ as one code point

type Case = {
  name: string;
  /** The word the cluster is in, which the sentence holds once. */
  word: string;
  /** The cluster: the cut offsets run across it. */
  cluster: string;
  sentence: string;
  /** A joiner (ZWJ or ZWNJ) is in the cluster. */
  joiner: boolean;
};

const CASES: Case[] = [
  {
    name: "conjunct क्ष",
    word: "परीक्षा",
    cluster: "क्ष",
    joiner: false,
    sentence: "कल की परीक्षा के लिए सभी छात्र तैयार रहें।",
  },
  {
    name: "conjunct त्र",
    word: "छात्र",
    cluster: "त्र",
    joiner: false,
    sentence: "छात्र संघ ने इस साल नया अध्यक्ष चुना है।",
  },
  {
    name: "conjunct ज्ञ",
    word: "ज्ञान",
    cluster: "ज्ञ",
    joiner: false,
    sentence: "इस ज्ञान से हमें बहुत लाभ होगा।",
  },
  {
    name: "conjunct श्र",
    word: "श्रमिक",
    cluster: "श्र",
    joiner: false,
    sentence: "मजदूरों के श्रमिक संघ ने काम बंद करने की घोषणा की।",
  },
  {
    name: "reph र्म in कर्म",
    word: "कर्म",
    cluster: "र्म",
    joiner: false,
    sentence: "यह कर्म हमारे कर्तव्य का हिस्सा है।",
  },
  {
    name: "reph र्क in कर्क",
    word: "कर्क",
    cluster: "र्क",
    joiner: false,
    sentence: "कर्क रेखा भारत के बीच से गुजरती है।",
  },
  {
    name: "nukta ज़ (decomposed)",
    word: `ज${NUKTA}मीन`,
    cluster: `ज${NUKTA}`,
    joiner: false,
    sentence: `उसने अपनी ज${NUKTA}मीन बेच दी।`,
  },
  {
    name: "nukta क़ (decomposed)",
    word: `क${NUKTA}ानून`,
    cluster: `क${NUKTA}`,
    joiner: false,
    sentence: `यह क${NUKTA}ानून अगले साल से लागू होगा।`,
  },
  {
    name: "nukta फ़ (decomposed)",
    word: `फ${NUKTA}ायदे`,
    cluster: `फ${NUKTA}`,
    joiner: false,
    sentence: `इस योजना के फ${NUKTA}ायदे साफ़ लिखे हैं।`,
  },
  {
    name: "nukta ज़ (precomposed)",
    word: `${PRECOMPOSED_NUKTA_JA}मीन`,
    cluster: PRECOMPOSED_NUKTA_JA,
    joiner: false,
    sentence: `उसने अपनी ${PRECOMPOSED_NUKTA_JA}मीन बेच दी।`,
  },
  {
    name: "ZWJ क्‍ष",
    word: `परीक्${ZWJ}षा`,
    cluster: `क्${ZWJ}ष`,
    joiner: true,
    sentence: `कल की परीक्${ZWJ}षा के लिए सभी छात्र तैयार रहें।`,
  },
  {
    name: "ZWNJ क्‌ष",
    word: `परीक्${ZWNJ}षा`,
    cluster: `क्${ZWNJ}ष`,
    joiner: true,
    sentence: `कल की परीक्${ZWNJ}षा के लिए सभी छात्र तैयार रहें।`,
  },
  {
    name: "matra े and anusvara after क्ष",
    word: "रक्षें",
    cluster: "क्षें",
    joiner: false,
    sentence: "सरकार नागरिकों की रक्षें सुनिश्चित करे।",
  },
  {
    name: "matra ी and anusvara after श्र",
    word: "श्रीं",
    cluster: "श्रीं",
    joiner: false,
    sentence: "पूजा में श्रीं मंत्र का जाप किया गया।",
  },
  {
    name: "matra ी and chandrabindu after श्र",
    word: "श्रीँ",
    cluster: "श्रीँ",
    joiner: false,
    sentence: "पूजा में श्रीँ मंत्र का जाप किया गया।",
  },
];

/** The four places a sentence sits in. */
const CONTEXTS: Record<string, (sentence: string) => string> = {
  "a paragraph": (sentence) => sentence,
  "a heading": (sentence) => `## ${sentence}`,
  "a list item": (sentence) => `- ${sentence}`,
  "a table cell": (sentence) => `| नाम | विवरण |\n| --- | --- |\n| ग्राहक | ${sentence} |`,
};

/** A node of mdast or hast, as far as the text goes. */
type Node = { type: string; value?: string; tagName?: string; children?: Node[] };

/** The block nodes of mdast and of hast: their text is set off from the next block's by a space. */
const MD_BLOCKS = new Set([
  "paragraph",
  "heading",
  "listItem",
  "list",
  "table",
  "tableRow",
  "tableCell",
]);
const HAST_BLOCKS = new Set([
  "p",
  "h1",
  "h2",
  "h3",
  "li",
  "ul",
  "ol",
  "table",
  "thead",
  "tbody",
  "tr",
  "th",
  "td",
]);

/** The text of a tree, its text nodes in order, a block's text set off from its neighbours by a space. */
function textOf(node: Node, blocks: Set<string>): string {
  if (node.type === "text" || node.type === "inlineCode") return node.value ?? "";
  const inner = (node.children ?? []).map((child) => textOf(child, blocks)).join("");
  return blocks.has(node.tagName ?? node.type) ? ` ${inner} ` : inner;
}

/** The text of a tree with no separators and with whitespace-only text nodes (a table's layout) left out. */
function rawOf(node: Node): string {
  if (node.type === "text" || node.type === "inlineCode")
    return /^\s*$/.test(node.value ?? "") ? "" : (node.value ?? "");
  return (node.children ?? []).map(rawOf).join("");
}

const normal = (text: string) => text.replace(/\s+/g, " ").trim();

/** An mdast tree's plain text: Markdown stripped, blocks set off, whitespace runs as one space. */
const plainMd = (root: Node) => normal(textOf(root, MD_BLOCKS));
/** An mdast tree's text as it reads, with no separators. */
const rawMd = (root: Node) => rawOf(root);
/** The views' hast, read once each (a view's hast is made again from its input each time it is read). */
const hastsOf = (views: readonly BlockView[]): Node[] => views.map((view) => view.hast as Node);
/** The views' plain text, as plainMd makes it of their source. */
const plainOf = (hasts: readonly Node[]) =>
  normal(hasts.map((hast) => textOf(hast, HAST_BLOCKS)).join(" "));
/** The views' text as it reads, with no separators (rawMd's form). */
const rawViews = (hasts: readonly Node[]) => hasts.map(rawOf).join("");

/** The grapheme boundaries of `text`: the starts of its graphemes, and its end. */
const SEGMENTER = new Intl.Segmenter(undefined, { granularity: "grapheme" });
function boundariesOf(text: string): Set<number> {
  const at = new Set<number>([text.length]);
  for (const { index } of SEGMENTER.segment(text)) at.add(index);
  return at;
}

/** A test's document, with its text as the finished render shows it and that text's grapheme boundaries. */
type Doc = { text: string; whole: string; boundaries: Set<number> };

function docOf(text: string): Doc {
  const whole = rawViews(hastsOf(createAnswer("p").update(text, false)));
  return { text, whole, boundaries: boundariesOf(whole) };
}

/**
 * Streams `doc` in the chunks ending at `cuts` (and then the whole), and expects each update to equal a fresh
 * answer's and to show the prefix's text, ending at the chunk's end; then the finished render to equal a one-shot
 * render. Returns the fast path's hits per update, and how many updates ended mid-grapheme.
 */
function stream(
  doc: Doc,
  cuts: readonly number[],
  label: string,
): { hits: number[]; midGrapheme: number } {
  const answer = createAnswer("p");
  const hits: number[] = [];
  let midGrapheme = 0;
  for (const end of [...cuts, doc.text.length]) {
    const prefix = doc.text.slice(0, end);
    const at = `${label} at ${end} (${JSON.stringify(prefix.slice(-12))})`;
    const counts = { parses: 0, chars: 0, fast: 0 };
    parseProbe.counts = counts;
    let views: readonly BlockView[];
    try {
      views = answer.update(prefix, true);
    } finally {
      parseProbe.counts = undefined;
    }
    hits.push(counts.fast);
    expectSame(views, createAnswer("p").update(prefix, true), at, true);
    const hasts = hastsOf(views);
    const plain = plainOf(hasts);
    expect(plain, at).not.toContain("\uFFFD");
    const oracle = parse(prefix);
    expect(plain, at).toBe(plainMd(oracle));
    const emitted = rawViews(hasts);
    expect(emitted, at).toBe(rawMd(oracle));
    expect(doc.whole.startsWith(emitted), at).toBe(true);
    if (!doc.boundaries.has(emitted.length)) midGrapheme++;
  }
  expectSame(
    answer.update(doc.text, false),
    createAnswer("p").update(doc.text, false),
    `${label} finished`,
    true,
  );
  return { hits, midGrapheme };
}

for (const kase of CASES)
  test(`${kase.name}: streamed split at every offset of the cluster, in four contexts, equals a fresh render`, () => {
    expect(kase.sentence.split(kase.word), kase.name).toHaveLength(2);
    expect(kase.word.split(kase.cluster), kase.name).toHaveLength(2);
    let midGrapheme = 0;
    let paragraphFast = 0;
    for (const [context, frame] of Object.entries(CONTEXTS)) {
      const doc = docOf(frame(kase.sentence));
      const start = doc.text.indexOf(kase.word) + kase.word.indexOf(kase.cluster);
      const end = start + kase.cluster.length;
      // A cut at 0 would stream an empty first chunk: every case's cluster is past the start of its text.
      expect(start, kase.name).toBeGreaterThan(0);
      for (let cut = start; cut <= end; cut++) {
        const label = `${kase.name} in ${context}, cut at ${cut}`;
        const { hits, midGrapheme: ended } = stream(doc, [cut], label);
        midGrapheme += ended;
        // Pinned: a paragraph's second update takes the fast path, unless the delta holds or follows a joiner.
        // Other contexts never do.
        const expected = context !== "a paragraph" ? 0 : kase.joiner && cut < end ? 0 : 1;
        expect(hits[1], label).toBe(expected);
        expect(hits[0], label).toBe(0);
        paragraphFast += context === "a paragraph" ? hits[1]! : 0;
      }
      if (kase.name.startsWith("ZWJ"))
        for (let first = start; first <= end; first++)
          for (let second = first + 1; second <= end; second++) {
            const label = `${kase.name} in ${context}, cuts at ${first} and ${second}`;
            midGrapheme += stream(doc, [first, second], label).midGrapheme;
          }
    }
    // The fast path is taken in every paragraph stream of a plain cluster, and after a joiner cluster's end.
    expect(paragraphFast, kase.name).toBeGreaterThan(0);
    if (kase.cluster.includes(NUKTA))
      // A base shows before its nukta arrives: the text ends inside a grapheme, at the chunk's end. (The
      // precomposed nukta is one code point, so no cut falls inside it.)
      expect(midGrapheme, kase.name).toBeGreaterThan(0);
  }, 30_000);

test("mid-stream, a cluster shows as far as it has arrived, with no holdback", () => {
  const answer = createAnswer("p");
  const shows = (prefix: string) => plainOf(hastsOf(answer.update(prefix, true)));
  // A conjunct's first half, then its second half: the virama shows once it has arrived.
  expect(shows("कल की परीक्")).toBe("कल की परीक्");
  expect(shows("कल की परीक्ष")).toBe("कल की परीक्ष");
  // A base, then its nukta: the base alone shows until the nukta arrives.
  expect(shows("उसने अपनी ज")).toBe("उसने अपनी ज");
  expect(shows(`उसने अपनी ज${NUKTA}`)).toBe(`उसने अपनी ज${NUKTA}`);
  // A joiner shows as the text it is, before the consonant after it arrives.
  expect(shows(`कल की परीक्${ZWJ}`)).toBe(`कल की परीक्${ZWJ}`);
});
