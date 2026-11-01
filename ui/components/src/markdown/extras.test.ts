// The syntax extras (syntax-extras.ts, mark-syntax.ts, definition lists) and heading anchors (slugs.ts):
// what each renders, what stays as written, the splitter never cutting a definition list, ids numbered
// across the answer and scoped to it, and the Indian case citations intact, finished and streamed.
import GithubSlugger from "github-slugger";
import { expect, test } from "vitest";
import { createAnswer } from "./answer.ts";
import { endOf, parse } from "./parse.ts";
import { slug } from "./slugs.ts";
import { htmlOf, normalise, unsplit } from "./spec-fixture.ts";
import { createSplitter } from "./split.ts";

const html = (text: string, streaming = false) =>
  htmlOf(createAnswer("t").update(text, streaming)).replace(/\n/g, "");
const tail = (text: string) =>
  htmlOf(createAnswer("t").update(text, true).slice(-1)).replace(/\n/g, "");
const split = (text: string) => createSplitter((block) => endOf(block, parse(block)))(text);

test("^superscript^: Pandoc's rule, no whitespace inside; footnotes and carets that close nothing stay", () => {
  expect(html("Section 3^A^ and the 2^nd^ notice")).toBe(
    "<p>Section 3<sup>A</sup> and the 2<sup>nd</sup> notice</p>",
  );
  expect(html("2^10 shares, and 2^10 and 3^2^")).toBe(
    "<p>2^10 shares, and 2^10 and 3<sup>2</sup></p>",
  );
  expect(html("^a b^ and ^^ and \\^a^ and a^[b]^")).toBe("<p>^a b^ and ^^ and ^a^ and a^[b]^</p>");
  expect(html("x[^a][^b] and [^c^]")).toBe("<p>x[^a][^b] and [^c^]</p>");
  expect(html("A[^n] 2^nd^.\n\n[^n]: The note.")).toContain("1</a></sup> 2<sup>nd</sup>.");
  expect(html("`2^10^` and [x^2^](https://ok.example/)")).toBe(
    '<p><code>2^10^</code> and <a href="https://ok.example/">x<sup>2</sup></a></p>',
  );
});

test("==highlight==: exactly two markers; a == b, === and a lone == stay as written", () => {
  expect(html("==The cap== and **==bold mark==** and ==*em*==")).toBe(
    "<p><mark>The cap</mark> and <strong><mark>bold mark</mark></strong> and <mark><em>em</em></mark></p>",
  );
  expect(html("a == b, x = 1, ===c=== and ==open")).toBe(
    "<p>a == b, x = 1, ===c=== and ==open</p>",
  );
  expect(html("Title\n===")).toBe('<h1 id="user-content-t-title">Title</h1>');
});

test("~sub~ is GFM strikethrough and ++ins++ is text: neither is supported syntax", () => {
  expect(html("H~2~O and C++ and ++new++")).toBe("<p>H<del>2</del>O and C++ and ++new++</p>");
});

test("while streaming, a half superscript is hidden and an open highlight shows highlighted", () => {
  expect(tail("the 2^nd")).toBe("<p>the 2</p>");
  expect(tail("the 2^nd^")).toBe("<p>the 2<sup>nd</sup></p>");
  expect(tail("2^10 ")).toBe("<p>2^10</p>");
  expect(tail("a ==marked")).toBe("<p>a <mark>marked</mark></p>");
  expect(tail("a ==marked=")).toBe("<p>a <mark>marked</mark></p>");
  expect(tail("a ==")).toBe("<p>a</p>");
  expect(tail("a == b")).toBe("<p>a == b</p>");
});

test("definition lists: terms, several definitions, a blank line before a definition, and in quotes", () => {
  expect(html("**Affiliate**\n: Any entity.\n: Or person.\n\nDay\n\n: A day.")).toBe(
    "<dl><dt><strong>Affiliate</strong></dt><dd>Any entity.</dd><dd>Or person.</dd><dt>Day</dt><dd><p>A day.</p></dd></dl>",
  );
  expect(html("> Term\n> : Meaning")).toBe(
    "<blockquote><dl><dt>Term</dt><dd>Meaning</dd></dl></blockquote>",
  );
  expect(html("Term\n:no space")).toBe("<p>Term:no space</p>");
});

test("the splitter never cuts inside a definition list, and blocks render as the whole answer does", () => {
  const texts = [
    "T1\n: d1\n\nT2\n: d2\n\nAfter.",
    "Term\n\n: definition\n\nNext",
    "Para.\n\nT1\n: d1\n\n    more of d1\n\nT2\n: d2",
    "# Defined terms\n\n**A**\n: one\n\n**B**\n: two\n\n- a list",
  ];
  for (const text of texts) {
    for (const block of split(text)) expect(block.trimStart(), text).not.toMatch(/^:[ \t]/);
    expect(split("T1\n: d1\n\nT2\n: d2\n\nAfter.")[0]).toContain("T2\n: d2");
    for (let end = 1; end <= text.length; end++) createAnswer("s").update(text.slice(0, end), true);
    expect(normalise(htmlOf(createAnswer("spec").update(text, false))), text).toBe(
      normalise(unsplit(text)),
    );
  }
});

test("slugs follow github-slugger on English, Hindi and Tamil headings, punctuation and emoji", () => {
  const headings = [
    "4.2 Termination",
    "धारा 4.2: समाप्ति",
    "பிரிவு 4.2 — முடிவு",
    "Clause 7(a) – Fees & Costs",
    "  Lead  and trailing  ",
    "Ünïcödé Straße",
    "C++ and C# 🎉",
    "x² and ½ and Ⅻ",
    "snake_case-and-dashes",
    "Notes",
    "Notes",
    "Notes-1",
    "",
  ];
  const slugger = new GithubSlugger();
  for (const heading of headings)
    expect(slug(heading), heading).toBe(new GithubSlugger().slug(heading));
  const shown = headings.filter((h) => h.trim() !== "");
  const views = createAnswer("t").update(shown.map((h) => `# ${h}`).join("\n\n"), false);
  const ids = htmlOf(views).match(/id="[^"]+"/g);
  expect(ids).toEqual(shown.map((h) => `id="user-content-t-${slugger.slug(h.trim())}"`));
});

test("heading ids are numbered across blocks, scoped per answer, and links to them resolve", () => {
  const text =
    "## 4.2 Termination\n\nSee [4.2](#42-termination).\n\n### Notes\n\nOne.\n\n### Notes\n\nTwo.";
  expect(html(text)).toContain('<h2 id="user-content-t-42-termination">');
  expect(html(text)).toContain('<h3 id="user-content-t-notes">');
  expect(html(text)).toContain('<h3 id="user-content-t-notes-1">');
  expect(htmlOf(createAnswer("other").update(text, false))).toContain(
    'id="user-content-other-notes-1"',
  );
  // The link's href (link.tsx maps it to the scoped id) is checked in the DOM by legal-extras.test.tsx.
  expect(html(text)).toContain('<a href="#42-termination">4.2</a>');
});

test("a renumbered heading's block keeps its view while the tail streams", () => {
  const answer = createAnswer("t");
  const first = answer.update("# A\n\ntext\n\n# A\n\nstreaming", true);
  const second = answer.update("# A\n\ntext\n\n# A\n\nstreaming on", true);
  expect(second[2]).toBe(first[2]);
  expect(htmlOf([second[2]!])).toContain('id="user-content-t-a-1"');
});

const CITATIONS = [
  "The rule in Kesavananda Bharati v State of Kerala, (1973) 4 SCC 225, was applied in (2019) 3 SCC 1; see also AIR 1950 SC 124, [1950] SCR 88 and 2019 SCC OnLine Del 1234.",
  "केशवानंद भारती बनाम केरल राज्य, (1973) 4 SCC 225, का सिद्धांत (2019) 3 SCC 1 में लागू हुआ; AIR 1950 SC 124, [1950] SCR 88 और 2019 SCC OnLine Del 1234 भी देखें।",
  "கேசவானந்த பாரதி எதிர் கேரள அரசு, (1973) 4 SCC 225, இல் உள்ள விதி (2019) 3 SCC 1 இல் பின்பற்றப்பட்டது; AIR 1950 SC 124, [1950] SCR 88 மற்றும் 2019 SCC OnLine Del 1234 ஆகியவற்றையும் பார்க்கவும்.",
  // At the start of a line, where a list or a reference could begin.
  "As held in\n(2019) 3 SCC 1 and\n2019 SCC OnLine Del 1234 and\n[1950] SCR 88 and\nAIR 1950 SC 124.",
];
const MANGLED = /<(?:a|sup|sub|em|strong|del|s|mark|ins|u|li|ol|ul|code|h\d)[ >]/;

test("Indian case citations stay intact: never links, footnotes, lists, superscripts, emphasis or strikethrough", () => {
  for (const text of CITATIONS) {
    const finished = html(text);
    expect(finished, text).not.toMatch(MANGLED);
    expect(finished, text).toBe(`<p>${text.replace(/\n/g, "")}</p>`);
    const answer = createAnswer("s");
    for (let end = 1; end <= text.length; end++) {
      const streamed = htmlOf(answer.update(text.slice(0, end), true));
      expect(streamed, text.slice(0, end)).not.toMatch(MANGLED);
    }
  }
});
