// The spec examples we deliberately render differently, one entry per example, each with its reason. The
// spec suite fails when an example not listed here fails, and when a listed one starts passing, so this
// list cannot go stale. Numbers are each spec's own example numbers (CommonMark 0.31.2; GFM 0.29-gfm).
// NOT_SUPPORTED, at the end, lists the syntax beyond the specs we decided not to support (#137 §3 and §4).
export type Reason = keyof typeof REASONS;
export type Exclusion = {
  spec: "commonmark" | "gfm";
  example: number;
  section: string;
  reason: Reason;
};

export const REASONS = {
  "raw-html":
    "Raw HTML (an HTML block or inline HTML) never becomes HTML, by design: the approved inline tags (sup, sub, u, ins, del, s, mark, br, small, kbd) paired within one parent become our elements with no attributes, and an HTML block of nothing but <br> line breaks; a tag with an HTML element's name that is clearly markup (paired, self-closed, or with a valued attribute) is dropped, its content kept as text; everything else shows as text (inline-tags.ts).",
  protocol:
    "The sanitiser drops an href whose scheme is not https, http, mailto, clauseo-cite or clauseo-paper (schema.ts); its check is case-sensitive, and a colon before any slash, ? or # counts as a scheme.",
  "gfm-autolink":
    "We render GFM, whose autolink literals link a bare URL or email address; the example's expected output is CommonMark's, without that extension.",
} as const;

export const EXCLUDED: readonly Exclusion[] = [
  { spec: "commonmark", example: 21, section: "Backslash escapes", reason: "raw-html" },
  {
    spec: "commonmark",
    example: 31,
    section: "Entity and numeric character references",
    reason: "raw-html",
  },
  { spec: "commonmark", example: 148, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 149, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 150, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 151, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 152, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 153, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 154, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 155, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 156, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 157, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 158, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 159, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 160, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 161, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 162, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 163, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 164, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 165, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 166, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 167, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 169, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 170, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 171, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 172, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 173, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 174, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 175, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 176, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 177, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 178, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 179, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 180, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 181, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 182, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 183, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 184, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 185, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 186, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 187, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 188, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 189, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 190, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 191, section: "HTML blocks", reason: "raw-html" },
  { spec: "commonmark", example: 201, section: "Link reference definitions", reason: "raw-html" },
  { spec: "commonmark", example: 308, section: "Lists", reason: "raw-html" },
  { spec: "commonmark", example: 309, section: "Lists", reason: "raw-html" },
  { spec: "commonmark", example: 344, section: "Code spans", reason: "raw-html" },
  { spec: "commonmark", example: 475, section: "Emphasis and strong emphasis", reason: "raw-html" },
  { spec: "commonmark", example: 476, section: "Emphasis and strong emphasis", reason: "raw-html" },
  { spec: "commonmark", example: 477, section: "Emphasis and strong emphasis", reason: "raw-html" },
  { spec: "commonmark", example: 491, section: "Links", reason: "raw-html" },
  { spec: "commonmark", example: 494, section: "Links", reason: "raw-html" },
  { spec: "commonmark", example: 500, section: "Links", reason: "protocol" },
  { spec: "commonmark", example: 524, section: "Links", reason: "raw-html" },
  { spec: "commonmark", example: 536, section: "Links", reason: "raw-html" },
  { spec: "commonmark", example: 596, section: "Autolinks", reason: "protocol" },
  { spec: "commonmark", example: 597, section: "Autolinks", reason: "protocol" },
  { spec: "commonmark", example: 598, section: "Autolinks", reason: "protocol" },
  { spec: "commonmark", example: 599, section: "Autolinks", reason: "protocol" },
  { spec: "commonmark", example: 601, section: "Autolinks", reason: "protocol" },
  { spec: "commonmark", example: 602, section: "Autolinks", reason: "gfm-autolink" },
  { spec: "commonmark", example: 606, section: "Autolinks", reason: "gfm-autolink" },
  { spec: "commonmark", example: 608, section: "Autolinks", reason: "gfm-autolink" },
  { spec: "commonmark", example: 611, section: "Autolinks", reason: "gfm-autolink" },
  { spec: "commonmark", example: 612, section: "Autolinks", reason: "gfm-autolink" },
  { spec: "commonmark", example: 613, section: "Raw HTML", reason: "raw-html" },
  { spec: "commonmark", example: 614, section: "Raw HTML", reason: "raw-html" },
  { spec: "commonmark", example: 615, section: "Raw HTML", reason: "raw-html" },
  { spec: "commonmark", example: 616, section: "Raw HTML", reason: "raw-html" },
  { spec: "commonmark", example: 617, section: "Raw HTML", reason: "raw-html" },
  { spec: "commonmark", example: 623, section: "Raw HTML", reason: "raw-html" },
  { spec: "commonmark", example: 625, section: "Raw HTML", reason: "raw-html" },
  { spec: "commonmark", example: 626, section: "Raw HTML", reason: "raw-html" },
  { spec: "commonmark", example: 627, section: "Raw HTML", reason: "raw-html" },
  { spec: "commonmark", example: 628, section: "Raw HTML", reason: "raw-html" },
  { spec: "commonmark", example: 629, section: "Raw HTML", reason: "raw-html" },
  { spec: "commonmark", example: 630, section: "Raw HTML", reason: "raw-html" },
  { spec: "commonmark", example: 631, section: "Raw HTML", reason: "raw-html" },
  { spec: "commonmark", example: 642, section: "Hard line breaks", reason: "raw-html" },
  { spec: "commonmark", example: 643, section: "Hard line breaks", reason: "raw-html" },
  { spec: "gfm", example: 628, section: "Autolinks (extension)", reason: "protocol" },
  { spec: "gfm", example: 652, section: "Disallowed Raw HTML (extension)", reason: "raw-html" },
];

/** Syntax beyond CommonMark and GFM that the renderer does not support, each with its reason (#137 §3, §4). */
export const NOT_SUPPORTED: readonly { syntax: string; shows: string; reason: string }[] = [
  {
    syntax: "~subscript~",
    shows: "strikethrough",
    reason:
      "GFM's strikethrough takes a single tilde (GFM spec, Strikethrough extension), so ~x~ must stay strikethrough. Subscript is written <sub>.",
  },
  {
    syntax: "++insert++",
    shows: "as written",
    reason:
      "It collides with C++ and increments, and no agent's prompt uses it. Insert and underline are written <ins> and <u>.",
  },
  {
    syntax: "^superscript with spaces^",
    shows: "as written",
    reason:
      "Pandoc's rule: nothing between the carets may be whitespace, so 2^10 and 3^2 never turns the text between into a superscript. Write <sup> for one with spaces.",
  },
  {
    syntax:
      "inline HTML beyond the allowlist (any tag but sup, sub, u, ins, del, s, mark, br, small, kbd), and every attribute",
    shows: "the tag as text; an allowlisted tag's attributes are dropped",
    reason:
      "Raw HTML stays out (research 16; G4's acceptance check 3 shows what rehype-raw lets through). Tags show as text, not dropped, because legal drafting uses placeholders like <Party A> and a lawyer must see everything the agent wrote.",
  },
  {
    syntax: "an HTML block other than one of nothing but <br>, <br/> or <br /> without attributes",
    shows: "as text",
    reason: "Block-level HTML is never rendered; only a line break on a line of its own is.",
  },
  {
    syntax: "a paragraph longer than 16 KB (split.ts's MAX_JOINED)",
    shows:
      "as paragraphs of about 4 KB each (MAX_PARAGRAPH), shown with no gap between them; an emphasis, link, tag pair or highlight spanning a cut shows its syntax as written",
    reason:
      "While streaming, every update parses the last block whole, so a long paragraph is cut into parts of MAX_PARAGRAPH (one 18 KB paragraph stalled a WebView2 flush for 1.2 s). A finished paragraph's parts are joined and parsed once, up to 16 KB: one join at 16 KB costs 22 to 51 ms in Node, one of 64 KB of links 455 ms, past what a real engine's frame budget allows. An answer's paragraphs are about 1 KB.",
  },
  {
    syntax: "a definition list's 33rd definition and on",
    shows: "as paragraphs, with a visible ': '",
    reason:
      "micromark-extension-definition-list parses a list in time quadratic in its terms (Node: 50 terms 19 ms, 300 terms 276 ms) and the splitter never cuts one, so nesting.ts caps a list at MAX_DEFINITIONS (32).",
  },
];
