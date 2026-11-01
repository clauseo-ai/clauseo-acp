// The sanitise schema (research/16-webview-security.md §3.5, as in the G4 spike), narrowed to what our
// pipeline emits: the tags and attributes mdast-util-to-hast produces for CommonMark and GFM (footnotes
// included), the syntax extras (definition lists, superscript, highlight) and the approved inline tags
// (inline-tags.ts), and nothing from GitHub's broad global list (`name`, `action`, `tabIndex`, `title`…):
// the inline tags carry no attribute at all, and headings only their generated id. Protocols: href https, http, mailto and our
// two reference schemes; src clauseo-file and data, the latter narrowed to raster data:image/ by the image
// component. rehype-sanitize's `user-content-` clobber prefix stays. Our components then decide what is
// clickable.
import { defaultSchema } from "hast-util-sanitize";
import type { Schema } from "hast-util-sanitize";

/** The prefix the sanitiser puts on every id and name, so content cannot clobber the page's globals. */
export const CLOBBER_PREFIX = "user-content-";

export const SCHEMA: Schema = {
  ancestors: defaultSchema.ancestors,
  clobber: defaultSchema.clobber,
  clobberPrefix: CLOBBER_PREFIX,
  required: defaultSchema.required,
  strip: defaultSchema.strip,
  protocols: {
    href: ["https", "http", "mailto", "clauseo-cite", "clauseo-paper"],
    src: ["clauseo-file", "data"],
  },
  tagNames: [
    "p",
    "blockquote",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "pre",
    "code",
    "br",
    "em",
    "strong",
    "del",
    "a",
    "img",
    "ul",
    "ol",
    "li",
    "input",
    "table",
    "thead",
    "tbody",
    "tr",
    "th",
    "td",
    "sup",
    // The syntax extras and the approved inline tags (sup, del and br are above).
    "sub",
    "u",
    "ins",
    "s",
    "mark",
    "small",
    "kbd",
    "dl",
    "dt",
    "dd",
  ],
  attributes: {
    // Links, and footnote references and backlinks.
    a: [
      "href",
      "id",
      "ariaDescribedBy",
      "dataFootnoteRef",
      "dataFootnoteBackref",
      ["className", "data-footnote-backref"],
    ],
    code: [["className", /^language-./]],
    // A heading's anchor (slugs.ts).
    h1: ["id"],
    h2: ["id"],
    h3: ["id"],
    h4: ["id"],
    h5: ["id"],
    h6: ["id"],
    img: ["src", "alt"],
    // Task list items.
    input: [["type", "checkbox"], ["disabled", true], "checked"],
    li: ["id", ["className", "task-list-item"]],
    ol: ["start", ["className", "contains-task-list"]],
    ul: [["className", "contains-task-list"]],
    // A footnote reference whose note has not arrived yet.
    sup: ["dataFootnotePending"],
    th: ["align"],
    td: ["align"],
  },
};

/** Link destinations the link component hands to the host's open_external. */
export const LINK_PREFIXES = ["https://", "http://", "mailto:"];
/** Our own references, rendered as citations and paper links, never as links. */
export const CITE_PREFIX = "clauseo-cite:";
export const PAPER_PREFIX = "clauseo-paper:";

/**
 * The id a citation or paper reference may carry: letters, digits, `_` and `-`, 1 to 128 of them. Anything
 * else (`clauseo-cite:javascript:…`, a path, a URL) is not a reference and renders as plain text.
 */
const REFERENCE_ID = /^[A-Za-z0-9_-]{1,128}$/;

export type Reference = { kind: "citation" | "paper"; id: string };

/** The reference an href names: a valid one, `"invalid"` for our scheme with a bad id, or null for a plain link. */
export function referenceOf(href: string | undefined): Reference | "invalid" | null {
  if (href === undefined) return null;
  const kind = href.startsWith(CITE_PREFIX)
    ? "citation"
    : href.startsWith(PAPER_PREFIX)
      ? "paper"
      : null;
  if (!kind) return null;
  const id = href.slice(kind === "citation" ? CITE_PREFIX.length : PAPER_PREFIX.length);
  return REFERENCE_ID.test(id) ? { kind, id } : "invalid";
}

/** An image of the matter's files. */
export const FILE_PREFIX = "clauseo-file:";

/**
 * Whether `src` names one of the matter's files: `clauseo-file:` and an opaque id with the reference
 * pattern (decision 0001: files are ids, never paths, so `clauseo-file:../../etc/passwd` is not one).
 */
export function isFileSource(src: string): boolean {
  return src.startsWith(FILE_PREFIX) && REFERENCE_ID.test(src.slice(FILE_PREFIX.length));
}
