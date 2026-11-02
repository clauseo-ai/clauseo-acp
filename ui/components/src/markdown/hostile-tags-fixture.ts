// Hostile cases for the approved inline tags and the syntax extras (#137 §3 and §4), added to the hostile
// corpus (hostile-cases.ts appends them, so the private real-engine harness runs them too). Payloads call
// window.__g4pwn(<case>) if they ever run. `tags` counts the allowlisted elements the finished render must
// hold (each with no attribute, which the suite checks for every case); `delta: 1` streams the case one
// character at a time, so every split of a tag across chunks is seen.
import type { HostileCase } from "./hostile-cases.ts";

const FORBIDDEN = [
  "script",
  "style",
  "iframe",
  "img",
  "a",
  "object",
  "embed",
  "svg",
  "math",
  "form",
  "input",
  "button",
  "textarea",
  "select",
  "option",
  "link",
  "meta",
  "base",
  "video",
  "audio",
  "details",
  "template",
  "div",
  "p",
  "span",
  "table",
  "blockquote",
  "h1",
  "ul",
  "li",
  "pre",
  "b",
  "i",
];

export const TAG_CASES: HostileCase[] = [
  {
    id: "tags-nested",
    markdown: "Rule 3<sup><sub>a</sub></sup> and <mark><del>old</del> <ins>new</ins></mark> text.",
    text: ["Rule 3a and old new text."],
    tags: { sup: 1, sub: 1, mark: 1, del: 1, ins: 1 },
    delta: 1,
  },
  {
    id: "tags-unclosed",
    markdown: "Fee <sup>2 and the rest of the clause continues to the end.",
    text: ["2 and the rest of the clause continues to the end."],
    tags: { sup: 0 },
    delta: 1,
  },
  {
    id: "tags-onclick",
    markdown: `See note <sup onclick="window.__g4pwn('tags-onclick')" class="x" id="y">1</sup>.`,
    tags: { sup: 1 },
    delta: 1,
  },
  {
    id: "tags-style",
    markdown: `<mark style="background:url(https://exfil.test/mark)">flagged</mark> and <kbd style="position:fixed">K</kbd>`,
    text: ["flagged"],
    tags: { mark: 1, kbd: 1 },
  },
  {
    id: "tags-slash-onload",
    markdown: `<sup/onload=window.__g4pwn('tags-slash-onload')>x</sup> after`,
    text: ["<sup/onload=", "after"],
    tags: { sup: 0 },
    delta: 1,
  },
  {
    id: "tags-newline-attribute",
    markdown: `Clause <sup\nonmouseover="window.__g4pwn('tags-newline-attribute')"\nclass="x">4</sup> holds.`,
    text: ["holds."],
    tags: { sup: 1 },
    delta: 1,
  },
  {
    id: "tags-case",
    markdown: "Section 3<SUP>A</SUP>, H<Sub>2</sUB>O and <KBD>Esc</KBD>.",
    tags: { sup: 1, sub: 1, kbd: 1 },
    delta: 1,
  },
  {
    id: "tags-entities",
    markdown:
      "&lt;sup&gt;a&lt;/sup&gt; &#60;sub&#62;b&#60;/sub&#62; &#x3C;mark>c&#x3C;/mark> &#X3c;br&#x3E;",
    text: ["<sup>a</sup>", "<sub>b</sub>", "<mark>c</mark>", "<br>"],
    tags: { sup: 0, sub: 0, mark: 0, br: 0 },
  },
  {
    id: "tags-forbidden",
    markdown:
      "Inline: " +
      FORBIDDEN.map(
        (tag) => `<${tag} onclick="window.__g4pwn('tags-forbidden')">${tag}</${tag}>`,
      ).join(" "),
    // Each pair is markup (an HTML element's name, a valued attribute): dropped, its content as text.
    text: [`Inline: ${FORBIDDEN.join(" ")}`],
    absent: ["onclick"],
  },
  {
    id: "tags-in-code",
    markdown: "Write `<sup>2</sup>` for a superscript, or:\n\n```html\n<sub>1</sub>\n```",
    text: ["<sup>2</sup>", "<sub>1</sub>"],
    tags: { sup: 0, sub: 0 },
  },
  {
    id: "tags-in-link-text",
    markdown:
      "[clause 4<sup>A</sup> of the Act](https://ok.example/act) and [x](javascript:window.__g4pwn('tags-in-link-text')<sup>1</sup>)",
    tags: { sup: 1 },
  },
  {
    id: "tags-in-table",
    markdown:
      "| Clause | Note |\n| --- | --- |\n| 4<sup>A</sup> | first<br>second <sup onclick=\"window.__g4pwn('tags-in-table')\">2 |",
    text: ["<sup onclick="],
    streamingText: ["second"],
    tags: { sup: 1, br: 1 },
    delta: 1,
  },
  {
    id: "tags-in-heading",
    markdown: "## Clause 4<sup>A</sup> <u>Fees</u>\n\nSee [the clause](#clause-4a-fees).",
    tags: { sup: 1, u: 1 },
  },
  {
    id: "tags-stray-closers",
    markdown: "Text </sup> more </mark></kbd> end </br> done",
    text: ["Text </sup> more </mark></kbd> end </br> done"],
    tags: { sup: 0, mark: 0, kbd: 0, br: 0 },
    delta: 1,
  },
  {
    id: "tags-misnested",
    markdown: "<sup><sub>a</sup></sub> and *b <mark>c* d</mark> and <u>e\n\nf</u>",
    text: ["<sub>a", "</sub>", "<mark>c", "d</mark>", "<u>e", "f</u>"],
    streamingText: ["</sub>", "d</mark>", "f</u>"],
    tags: { sup: 1, sub: 0, mark: 0, u: 0 },
    delta: 1,
  },
  {
    id: "tags-spaced-and-lookalike",
    markdown:
      "< sup>a</ sup> <\u0455up>b</\u0455up> <s\u1d1cp>c</s\u1d1cp> \uff1csup\uff1ed\uff1c/sup\uff1e <sup >e</sup >",
    text: ["< sup>a</ sup>", "<\u0455up>b", "<s\u1d1cp>c", "\uff1csup\uff1ed"],
    tags: { sup: 1 },
    delta: 1,
  },
  {
    id: "tags-breaks",
    markdown:
      'a<br>b<br/>c<br />d<BR>e</br>f<br class="x" onclick="window.__g4pwn(\'tags-breaks\')">g<br/>',
    text: ["</br>"],
    tags: { br: 6 },
    delta: 1,
  },
  {
    id: "tags-self-closed",
    markdown: "x<sup/>y<mark />z",
    text: ["x<sup/>y<mark />z"],
    tags: { sup: 0, mark: 0 },
    delta: 1,
  },
  {
    id: "tags-block-html",
    markdown:
      "Before.\n\n<br>\n<br/>\n\n<sup>\nnot a tag pair\n</sup>\n\n<div onclick=\"window.__g4pwn('tags-block-html')\">\n<sup>1</sup>\n</div>\n\n<br>\nShown, not swallowed <sup>2</sup>\n\nAfter.",
    // The last HTML block holds a <br> and more: all of it shows as text, nothing is swallowed. The div's
    // tags are markup, dropped; the allowlisted tags in an HTML block stay text.
    text: ["<sup>\nnot a tag pair", "\n<sup>1</sup>\n", "<br>\nShown, not swallowed <sup>2</sup>"],
    absent: ["<div onclick="],
    tags: { br: 2, sup: 0 },
  },
  {
    id: "tags-in-footnote",
    markdown:
      "A claim[^n].\n\n[^n]: See <sup>2</sup> and <sup onclick=\"window.__g4pwn('tags-in-footnote')\">3</sup>.",
    tags: { "li sup": 2 },
  },
  {
    // Mid-stream, an unpaired tag is hidden only where its closer can still arrive: not inside an
    // emphasis that has closed.
    id: "tags-in-closed-emphasis-streamed",
    markdown: "*a <sup>b* c, and the answer goes on.",
    text: ["<sup>b"],
    tags: { sup: 0 },
  },
  {
    id: "extras-caret-and-equals",
    markdown: "2^10 and [^x] and a^b^c and ==mark==ed and a == b and ===c=== and x^<sup>y</sup>^",
    text: ["2^10 and [^x]", "a == b", "===c==="],
    streamingText: ["a == b", "===c==="],
    tags: { mark: 1, sup: 2 },
    delta: 1,
  },
  // HTML-named tags that are not on the list (inline-tags.ts): dropped, with their attributes, only when
  // clearly markup; placeholders that share an HTML element's name stay as written.
  {
    id: "tags-html-placeholders",
    markdown:
      "Send it to <Address>.\n\n<Title> of <Section> 4\n\nServe <Address of Tenant> by post.\n\n" +
      "Signed by <section><Party A></section> on <time><insert date></time>.",
    text: [
      "Send it to <Address>.",
      "<Title> of <Section> 4",
      "Serve <Address of Tenant> by post.",
      "Signed by <Party A> on <insert date>.",
    ],
    absent: ["<section>", "</section>", "<time>", "</time>"],
  },
  {
    id: "tags-html-nested-placeholders",
    markdown:
      "<div><span><Party A></span></div>\n\nBetween <div><span><Party B></span></div> and <Client>.",
    text: ["<Party A>", "Between <Party B> and <Client>."],
    absent: ["<div>", "<span>", "</span>", "</div>"],
  },
  {
    id: "tags-html-codex-divs",
    markdown:
      "| Point | Cap (₹) |\n|---|---:|\n" +
      '| General liability cap | <div align="right">A: 1× 12-month fees<br>B: 2× contract value</div> |\n' +
      '| कुल देयता की सीमा | <div align="right">A: ₹1 करोड़ तक<br>B: ₹1 करोड़ तक</div> |',
    text: ["A: 1× 12-month fees", "B: 2× contract value", "A: ₹1 करोड़ तक"],
    absent: ["<div", "</div>", "align"],
    tags: { br: 2, "td div": 0 },
  },
  {
    id: "tags-html-mixed-case",
    markdown: `x <DiV class="q">y</dIv> z <SCRIPT>window.__g4pwn('tags-html-mixed-case')</SCRIPT> w <Center>c</CENTER>`,
    text: ["x y z", "window.__g4pwn('tags-html-mixed-case')", "w c"],
    // Mid-stream, tail repair may close the script's `__` as strong, which its text then lacks.
    streamingText: ["x y z", "w c"],
    absent: ["<DiV", "</dIv>", "<SCRIPT>", "</SCRIPT>", "<Center>"],
    tags: { "p *": 0 },
  },
  {
    id: "tags-html-split",
    markdown:
      'Cap: <div align="right">A: one<br>B: two</div> for <span><Party A></span>, <SPAN>kept</SPAN>.\n\n' +
      "<script>window.__g4pwn('tags-html-split')</script> and <style>p { display: none }</style>",
    text: [
      "Cap: A: one",
      "B: two for <Party A>, kept.",
      "window.__g4pwn('tags-html-split') and p { display: none }",
    ],
    streamingText: [],
    absent: ["<div", "<span>", "<SPAN>", "<script>", "<style>"],
    tags: { br: 1 },
    delta: 1,
  },
  {
    id: "tags-html-attributes",
    markdown:
      `Text <div class="a" id="x" style="position:fixed" onclick="window.__g4pwn('tags-html-attributes')"\n` +
      `  onmouseover='window.__g4pwn("tags-html-attributes")' data-x="a>b" title='c > d' hidden>held</div> end.\n\n` +
      `<div onload="window.__g4pwn('tags-html-attributes')" data-q="</div>"\n  title=">">block</div>`,
    text: ["Text held end.", "block"],
    absent: ["onclick", "onmouseover", "data-x", "onload", "<div"],
  },
  {
    id: "tags-html-script-style",
    markdown:
      "<script>window.__g4pwn('tags-html-script-style')</script>\n\n<style>body { display: none }</style>\n\n" +
      "Inline <script>window.__g4pwn('tags-html-script-style')</script> and <style>p{color:red}</style>, " +
      "<iframe src=x>frame <b>t</b></iframe> <svg><script>window.__g4pwn('tags-html-script-style')</script></svg>.",
    text: [
      "window.__g4pwn('tags-html-script-style')",
      "body { display: none }",
      "Inline window.__g4pwn('tags-html-script-style') and p{color:red}, frame <b>t</b> <script>window.__g4pwn('tags-html-script-style')</script>.",
    ],
    streamingText: ["body { display: none }"],
    absent: ["<style>", "<iframe", "<svg>"],
    tags: { "p *": 0 },
  },
  // Denial of service: each must stay within hostile-time.test.ts's ratio to ordinary prose.
  {
    id: "tags-nested-5000",
    markdown: "<sup>".repeat(5000) + "x" + "</sup>".repeat(5000),
    text: ["<sup><sup><sup>", "</sup></sup></sup>"],
    streamingText: [],
  },
  { id: "tags-10000", markdown: "<u>a</u>".repeat(5000), tags: { u: 5000 } },
  {
    id: "tags-unclosed-5000",
    markdown: "<u>x ".repeat(5000),
    text: ["<u>x <u>x"],
    streamingText: [],
  },
  {
    id: "tags-closers",
    markdown: "<s><u><sub>".repeat(1000) + "</u>".repeat(3000),
    text: ["</u></u>"],
    streamingText: [],
  },
  { id: "extras-carets", markdown: "^a".repeat(8000) },
  // 400 carets, each a delimiter sequence, are as many as a paragraph keeps (nesting.ts): 200 superscripts,
  // the rest as written.
  { id: "extras-superscripts-6000", markdown: "^a^".repeat(6000), tags: { sup: 200 } },
  { id: "headings-5000", markdown: "# a\n".repeat(5000), tags: { h1: 5000 } },
  { id: "extras-caret-long", markdown: "2^" + "a".repeat(20000) },
  { id: "extras-equals", markdown: "==a ".repeat(5000) },
  { id: "extras-equals-pairs", markdown: "a==b==".repeat(3000) },
  {
    id: "extras-definitions",
    markdown: Array.from(
      { length: 300 },
      (_, index) => `Term ${index}\n: Definition ${index}.`,
    ).join("\n\n"),
  },
  { id: "extras-definition-markers", markdown: "Term\n" + ": ".repeat(5000) + "x" },
];
