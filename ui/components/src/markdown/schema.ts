// The sanitise schema (research/16-webview-security.md §3.5, as in the G4 spike): GitHub's default
// schema with the protocol lists narrowed (href https, http, mailto and our two reference schemes; src
// clauseo-file and data, the latter narrowed to raster data:image/ by the image component), the global
// `title` attribute dropped, and rehype-sanitize's `user-content-` clobber prefix kept. Our components
// then decide what is clickable.
import { defaultSchema } from "hast-util-sanitize";
import type { Schema } from "hast-util-sanitize";

type Attributes = NonNullable<Schema["attributes"]>;

function withoutTitle(attributes: Attributes): Attributes {
  const out: Attributes = {};
  for (const [tag, list] of Object.entries(attributes)) {
    out[tag] = list.filter((entry) => (Array.isArray(entry) ? entry[0] : entry) !== "title");
  }
  return out;
}

/** The prefix the sanitiser puts on every id and name, so content cannot clobber the page's globals. */
export const CLOBBER_PREFIX = "user-content-";

export const SCHEMA: Schema = {
  ...defaultSchema,
  clobberPrefix: CLOBBER_PREFIX,
  protocols: {
    ...defaultSchema.protocols,
    href: ["https", "http", "mailto", "clauseo-cite", "clauseo-paper"],
    src: ["clauseo-file", "data"],
  },
  attributes: withoutTitle(defaultSchema.attributes ?? {}),
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
