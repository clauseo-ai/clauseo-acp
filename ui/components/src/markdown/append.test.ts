// The append fast path (append.ts): it must take what is provably inert text and nothing else, and what it
// makes must be what a fresh parse and a fresh render make. The edges first, one by one; then random appends
// against micromark; then random streams against a fresh answer at every update (append-fixture.ts).
import { expect, test } from "vitest";
import { createAnswer } from "./answer.ts";
import { appended, appendProbe } from "./append.ts";
import {
  againstFresh,
  expectSame,
  BREAKS,
  deltasFor,
  DELTAS,
  lineText,
  MARKUP,
  newTotals,
  PLAIN_WORDS,
  randomOf,
  streamText,
} from "./append-fixture.ts";
import { loadEntities } from "./entities.ts";
import { parse, parseProbe } from "./parse.ts";

await loadEntities();

/** The fast path's mdast for `old` then `delta`, after checking it is micromark's, or undefined. */
function extend(old: string, delta: string) {
  const source = old + delta;
  const root = appended({ source: old, root: parse(old) }, source);
  if (root) expect(root, JSON.stringify(source)).toEqual(parse(source));
  return root;
}

test("plain prose appended to a paragraph's last text extends it, as micromark parses it", () => {
  for (const [old, delta] of [
    ["The Supplier shall", " notify the Client"],
    ["A first line\nand a second", " that runs on."],
    ["Ends with spaces  ", "and more"],
    ["**Clause 8.3** obliges the Supplier", " to pass it on (in writing)."],
    ["An *open emphasis and", " more words"],
    ["After ~~gone~~ and ==marked== and `code` it", " goes on"],
    ["A <b tag with no end", " yet"],
    ["See [x] and [^1] then", " more"],
    ["[x]: https://example.com/x\n[^1]: A note.\n\nSee [x] and [^1] then", " more"],
    ["An `unclosed backtick", " still text"],
    ["अनुबंध की शर्तें", " लागू होंगी।"],
    ["ஒப்பந்தம் விதிகள்", " பொருந்தும்."],
    ["Almost a link: ww", "w and http: and xmpp"],
    ["A [settled](/a) link then", " text"],
    ["[a]: /b\n\nSee [a] then", " more"],
    ["Text 𝐀𝐁", "𝐂 more"],
    ["A first line\n*x and", " more"],
    ["A first line\n**x and", " more"],
    ["A first line\n***x and", " more"],
    ["A first line\n_x and", " more"],
    ["A first line\n~~x and", " more"],
    ["**Clause 8.3** obliges", " the Supplier"],
    ["A first line\n    four spaces in", " a paragraph"],
  ] as const) {
    expect(extend(old, delta), JSON.stringify(old + delta)).toBeDefined();
  }
});

test("anything that may not be inert text falls back to a full parse", () => {
  for (const [old, delta] of [
    ["A paragraph", "\nand a line"], // a line ending
    ["A paragraph\n", "and a line"], // after a line start
    ["A paragraph", " with *emphasis"], // a delimiter in the delta
    ["A paragraph *", "x"], // the character before the delta is syntax
    ["Text &amp", ";"], // a character reference ends
    ["Text &amp; more", " text"], // `&` on the line
    ["Mail a@b.co", "m"], // an email literal's domain runs on
    ["Go to www", ".example.com"], // a www autolink
    ["Go to www.", "example.com"],
    ["See WWW.", "example"],
    ["See http:", "//example.com"], // a protocol autolink
    ["Write mailto:", "x"],
    ["Write xmpp:", "x"],
    ["An escape \\", "("], // an escape
    ['A [link](b "title', '")'], // an inline link's title closes
    ["A [link](", "b)"],
    ['x [a [b](c "t](d) more', '")'], // a link inside a link closes as its own
    ["x ![a [b](c 't](d) more", "')"],
    ['[a]: b\n"multi\nline', '"'], // a definition's title closes
    ["[a]: b\n(multi\nline", ")"],
    ["[a]: b\n'multi\nline", "'"],
    ["[a]:\nb\n(multi\nline", ")"],
    ["[a]:\nb", ' "title"'], // a definition still arriving
    ['[a]:\nb "ti', 'tle"'],
    ["- an item", " goes on"], // not a paragraph
    ["1. an item", " goes on"],
    ["> a quote", " goes on"],
    ["# A heading", " goes on"],
    ["Term\n: def", " goes on"], // a definition list
    ["First\n-", "-"], // not a letter first
    ["First\n(a", ") goes on"],
    ["First\n~~~x", " goes on"], // a fence
    ["First\n* x", " goes on"], // a list
    ["***", " x"], // a thematic break
    ["First\n***", "x"],
    ["    code", " goes on"], // indented code
    ["Ends with a [", "link"], // the character before is `[`
    ["A paragraph", ""], // nothing appended
  ] as const) {
    expect(extend(old, delta), JSON.stringify(old + delta)).toBeUndefined();
  }
});

test("over random appends, what the fast path makes is always micromark's mdast", () => {
  const random = randomOf(292);
  let made = 0;
  const cases = 4_000;
  for (let index = 0; index < cases; index++) {
    const old = streamText(random, 20 + Math.floor(random() * 200)).trimEnd();
    let delta = "";
    const parts = 1 + Math.floor(random() * 4);
    for (let part = 0; part < parts; part++) {
      const roll = random();
      const from = roll < 0.8 ? PLAIN_WORDS : roll < 0.95 ? MARKUP : BREAKS;
      delta += `${random() < 0.5 ? " " : ""}${from[Math.floor(random() * from.length)]!}`;
    }
    if (extend(old, delta)) made++;
  }
  // The edges are reached: a good share of the random appends were taken (and matched micromark's).
  console.log(`append: the fast path took ${made} of ${cases} random appends`);
  expect(made).toBeGreaterThan(cases / 10);
});

test("over random appends to a paragraph's last line, what the fast path makes is micromark's mdast", () => {
  const random = randomOf(2292);
  let made = 0;
  const cases = 6_000;
  for (let index = 0; index < cases; index++) {
    let delta = "";
    for (let part = 1 + Math.floor(random() * 3); part > 0; part--)
      delta += `${random() < 0.3 ? " " : ""}${DELTAS[Math.floor(random() * DELTAS.length)]!}`;
    if (extend(lineText(random), delta)) made++;
  }
  console.log(`append: the fast path took ${made} of ${cases} random appends to a last line`);
  expect(made).toBeGreaterThan(cases / 10);
});

test("random streams render, at every update, what a fresh answer renders", () => {
  const random = randomOf(1292);
  const totals = newTotals();
  for (let stream = 0; stream < 50; stream++) {
    const text = streamText(random, 150 + Math.floor(random() * 350));
    againstFresh(text, deltasFor(random, text.length), totals);
  }
  console.log(
    `append: ${totals.fastUpdates} of ${totals.updates} random-stream updates took the fast path`,
  );
  expect(totals.fastUpdates).toBeGreaterThan(totals.updates / 10);
}, 120_000);

/**
 * The edges of what the fast path takes, each in a paragraph streamed four ways: whether an update whose
 * tail's last line holds the edge took the fast path (`taken`), and that every update equals a fresh render.
 * An opener still waiting for its closer is pending for tail repair, so only closed ones are taken.
 */
const EDGES: { edge: string; taken: boolean; before?: string }[] = [
  ...[
    "*a* ",
    "**bold** ",
    "_u_ ",
    "~~s~~ ",
    "==hi== ",
    "^2^ ",
    "`code` ",
    "<b>x</b> ",
    "<b ",
    "<tag ",
  ].map((edge) => ({ edge, taken: true })),
  { edge: "[x] ", taken: true },
  { edge: "[x] ", taken: true, before: "[x]: https://example.com/x\n\n" },
  { edge: "[^1] ", taken: true },
  { edge: "[^1] ", taken: true, before: "[^1]: The note.\n\n" },
  ...["  ", "अनुबंध की शर्तें ", "ஒப்பந்தம் விதிகள் ", "ww.", "http: ", "WW.", "ww.w "].map((edge) => ({
    edge,
    taken: true,
  })),
  ...["\n*x* ", "\n**x** ", "\n***x*** ", "\n_x_ ", "\n~~x~~ ", "\n**Clause 8.3** obliges "].map(
    (edge) => ({
      edge,
      taken: true,
    }),
  ),
  ...[
    "*open ",
    "`open ",
    '<sup class="x ',
    "www.",
    "http://",
    "xmpp:",
    "mailto:",
    "&amp",
    "&amp; ",
    "a@b.co ",
  ].map((edge) => ({ edge, taken: false })),
  ...["\\* ", '[a](b "t ', "\n~~~x ", "\n* x ", "\n***\n", "\n- x ", "\n1. x ", "\n> x "].map(
    (edge) => ({
      edge,
      taken: false,
    }),
  ),
];

test("each edge of the fast path is taken or not as it should be, and renders what a fresh answer renders", () => {
  const random = randomOf(2920);
  const totals = newTotals();
  const report: string[] = [];
  for (const { edge, taken, before = "" } of EDGES) {
    const text = `${before}Some plain words come first ${edge}and plain words after it run on, for a while.`;
    const mark = edge.trim().split("\n").at(-1) || edge;
    let hits = 0;
    for (let split = 0; split < 4; split++) {
      const answer = createAnswer("p");
      for (let end = 0; end < text.length;) {
        end = Math.min(text.length, end + 1 + Math.floor(random() * 9));
        const prefix = text.slice(0, end);
        const counts = { parses: 0, chars: 0, fast: 0 };
        parseProbe.counts = counts;
        const views = answer.update(prefix, true);
        parseProbe.counts = undefined;
        totals.updates++;
        if (counts.fast > 0) totals.fastUpdates++;
        const line = prefix.slice(prefix.lastIndexOf("\n") + 1);
        if (counts.fast > 0 && line.includes(mark) && !line.endsWith(mark)) hits++;
        expectSame(views, createAnswer("p").update(prefix, true), JSON.stringify(prefix), true);
      }
    }
    report.push(`${JSON.stringify(before + edge)} ${hits}`);
    if (taken) expect(hits, JSON.stringify(before + edge)).toBeGreaterThan(0);
    else expect(hits, JSON.stringify(before + edge)).toBe(0);
  }
  console.log(
    `append edges: ${totals.fastUpdates} of ${totals.updates} updates took the fast path; hits with the edge on the line: ${report.join(", ")}`,
  );
}, 120_000);

test("a plain paragraph streams with no parse after its first update: one fast-path hit each", () => {
  const answer = createAnswer("c");
  const words = "The Supplier shall notify the Client within ten days of the change, in writing. ";
  const text = words.repeat(8);
  const perUpdate: { parses: number; fast: number }[] = [];
  for (let end = 9; end <= text.length; end += 9) {
    const counts = { parses: 0, chars: 0, fast: 0 };
    parseProbe.counts = counts;
    answer.update(text.slice(0, end), true);
    parseProbe.counts = undefined;
    perUpdate.push({ parses: counts.parses, fast: counts.fast });
  }
  expect(perUpdate[0]).toEqual({ parses: 1, fast: 0 });
  // An update whose delta ends in the space before a word is parsed whole only if its repair has something
  // pending; prose has none.
  expect(perUpdate.slice(1).every((one) => one.parses === 0 && one.fast === 1)).toBe(true);
});

test("the finished render never uses an mdast the fast path made", () => {
  // A wrong one stands in for each: the streaming render shows it, the finished one must not.
  appendProbe.replace = (root) => {
    const wrong = structuredClone(root);
    const paragraph = wrong.children.at(-1);
    const text = paragraph?.type === "paragraph" ? paragraph.children.at(-1) : undefined;
    if (text?.type === "text") text.value = "WRONG";
    return wrong;
  };
  try {
    // The last streaming update holds the whole text, so the finished one parses the same input.
    const text = "The Supplier shall notify the Client within ten days, in writing.";
    const answer = createAnswer("w");
    let shown = "";
    for (let end = 9; end < text.length + 9; end += 9)
      shown = JSON.stringify(answer.update(text.slice(0, end), true).at(0)!.content);
    expect(shown).toContain("WRONG");
    const once = createAnswer("w").update(text, false);
    expectSame(answer.update(text, false), once, "finished", true);
  } finally {
    appendProbe.replace = undefined;
  }
});
