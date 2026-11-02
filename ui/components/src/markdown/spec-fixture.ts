// Test support for the spec suites (not product code): the examples, the streaming run that feeds one
// answer model in chunks, and the HTML normalisation both sides go through before they are compared.
//
// Normalisation, in the spirit of the CommonMark repository's test/normalize.py: both HTML strings are
// parsed by an HTML5 parser (so quoting, entities, `<br />` against `<br>` and attribute order stop
// mattering), whitespace outside <pre> collapses to one space, and whitespace next to block-level tags
// and at the edges of block-level elements goes. Two attribute differences are ours by design, so the
// expected side loses them too: the `title` attribute (our sanitiser drops it everywhere) and remark's
// task-list classes (`contains-task-list`, `task-list-item`), which GFM's reference output does not have.
import type { ElementContent, Properties, Root, RootContent } from "hast";
import { fromHtml } from "hast-util-from-html";
import { sanitize } from "hast-util-sanitize";
import { toHtml } from "hast-util-to-html";
import { toHast } from "mdast-util-to-hast";
import { createAnswer, HANDLERS } from "./answer.ts";
import { markCutTags, pairTags } from "./inline-tags.ts";
import { capNesting, hasStandIns, restoreText } from "./nesting.ts";
import { parse } from "./parse.ts";
import { SCHEMA } from "./schema.ts";
import { anchorHeadings, headingId, numberSlugs, withSlugs } from "./slugs.ts";

export type Example = { example: number; section: string; markdown: string; html: string };

/** The examples in a CommonMark-style spec text, numbered as the spec numbers them; → stands for a tab. */
export function examplesOf(spec: string): Example[] {
  const body = spec.replace(/\r\n?/g, "\n").replace(/^<!-- END TESTS -->[\s\S]*/m, "");
  const pattern = /^`{32} example[^\n]*\n([\s\S]*?)^\.\n([\s\S]*?)^`{32}$|^#{1,6} *(.*)$/gm;
  const examples: Example[] = [];
  let section = "";
  for (const match of body.matchAll(pattern)) {
    if (match[3] !== undefined) {
      section = match[3];
      continue;
    }
    examples.push({
      example: examples.length + 1,
      section,
      markdown: (match[1] ?? "").replace(/→/g, "\t"),
      html: (match[2] ?? "").replace(/→/g, "\t"),
    });
  }
  return examples;
}

/** Tabs for the arrows of examples kept verbatim (gfm-spec.json). */
export function withTabs(example: Example): Example {
  return {
    ...example,
    markdown: example.markdown.replace(/→/g, "\t"),
    html: example.html.replace(/→/g, "\t"),
  };
}

const BLOCK = new Set(
  (
    "address article aside blockquote body caption center col colgroup dd details dialog dir div dl dt " +
    "fieldset figcaption figure footer form frame frameset h1 h2 h3 h4 h5 h6 head header hr html iframe " +
    "legend li link main menu menuitem meta nav noframes ol optgroup option p param pre section source " +
    "summary table tbody td tfoot th thead title tr track ul"
  ).split(" "),
);
const VOID = new Set(
  "area base br col embed hr img input link meta param source track wbr".split(" "),
);
const OURS = new Set(["contains-task-list", "task-list-item"]);

/** A heading's anchor (slugs.ts), which is ours: the spec's headings have none. */
const HEADING = /^h[1-6]$/;

function attributes(properties: Properties, name: string, anchors: boolean): string {
  const out: string[] = [];
  for (const key of Object.keys(properties).sort()) {
    if (key === "title") continue;
    if (key === "id" && !anchors && HEADING.test(name)) continue;
    let value = properties[key];
    if (key === "className" && Array.isArray(value)) {
      value = value.filter((name) => !OURS.has(String(name)));
      if (value.length === 0) continue;
    }
    if (value === false || value === null || value === undefined) continue;
    const text = value === true ? "" : Array.isArray(value) ? value.join(" ") : String(value);
    out.push(` ${key}="${text.replace(/&/g, "&amp;").replace(/"/g, "&quot;")}"`);
  }
  return out.join("");
}

const escape = (text: string) =>
  text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

function isBlock(node: RootContent | ElementContent | undefined): boolean {
  return node?.type === "element" && BLOCK.has(node.tagName);
}

function children(
  nodes: readonly (RootContent | ElementContent)[],
  inPre: boolean,
  inBlock: boolean,
  anchors: boolean,
): string {
  let out = "";
  nodes.forEach((node, index) => {
    if (node.type === "text") {
      if (inPre) {
        out += escape(node.value);
        return;
      }
      let text = node.value.replace(/\s+/g, " ");
      if (isBlock(nodes[index - 1]) || (index === 0 && inBlock)) text = text.trimStart();
      if (isBlock(nodes[index + 1]) || (index === nodes.length - 1 && inBlock))
        text = text.trimEnd();
      out += escape(text);
    } else if (node.type === "element") {
      const name = node.tagName;
      const open = `<${name}${attributes(node.properties, name, anchors)}>`;
      if (VOID.has(name)) out += open;
      else
        out += `${open}${children(node.children, inPre || name === "pre", BLOCK.has(name), anchors)}</${name}>`;
    } else if (node.type === "comment") {
      out += `<!--${node.value}-->`;
    }
  });
  return out;
}

/** The canonical form of an HTML fragment; without `anchors`, headings lose their ids (the spec has none). */
export function normalise(html: string, anchors = true): string {
  const tree: Root = fromHtml(html, { fragment: true });
  return children(tree.children, false, true, anchors);
}

/** A note's back-references with remark-rehype's English label, as the link component gives them. */
function withLabels(node: ElementContent): ElementContent {
  if (node.type !== "element") return node;
  const backref = node.properties.dataFootnoteBackref;
  const properties =
    typeof backref === "string" && backref !== ""
      ? { ...node.properties, dataFootnoteBackref: "", ariaLabel: `Back to reference ${backref}` }
      : node.properties;
  return { ...node, properties, children: node.children.map(withLabels) };
}

type View = { hast: Root; items?: readonly { hast: Root }[] | undefined };

/**
 * The notes section the renderer puts around the notes view's items (notes.tsx), as hast: remark-rehype's
 * footer, its heading's id scoped as each item's is.
 */
function notesSection(items: readonly { hast: Root }[]): Root {
  const children: ElementContent[] = [{ type: "text", value: "\n" }];
  for (const item of items)
    for (const child of item.hast.children)
      if (child.type === "element") children.push(withLabels(child), { type: "text", value: "\n" });
  const first = children.find((child) => child.type === "element");
  const id = first?.type === "element" ? String(first.properties.id ?? "") : "";
  const scope = id.replace(/^user-content-/, "").replace(/-fn-.*$/, "");
  const heading: ElementContent = {
    type: "element",
    tagName: "h2",
    properties: { className: ["sr-only"], id: `user-content-${scope}-footnote-label` },
    children: [{ type: "text", value: "Footnotes" }],
  };
  const list: ElementContent = { type: "element", tagName: "ol", properties: {}, children };
  return {
    type: "root",
    children: [
      {
        type: "element",
        tagName: "section",
        properties: { dataFootnotes: true, className: ["footnotes"] },
        children: [heading, { type: "text", value: "\n" }, list, { type: "text", value: "\n" }],
      },
    ],
  };
}

/** The HTML of the answer model's final hast: each block's sanitised hast, in order, then the notes. */
export function htmlOf(views: readonly View[]): string {
  return views.map((view) => toHtml(view.items ? notesSection(view.items) : view.hast)).join("\n");
}

/**
 * The markdown fed into one answer model `size` characters at a time while streaming (0: all at once),
 * then the whole text with streaming over; the HTML of the final render.
 */
export function streamed(markdown: string, size: number): string {
  const answer = createAnswer("spec");
  if (size > 0)
    for (let end = size; end < markdown.length; end += size)
      answer.update(markdown.slice(0, end), true);
  return htmlOf(answer.update(markdown, false));
}

/** The chunk sizes each example is streamed in: one character at a time for short ones, then fixed chunks. */
export function chunkingsFor(markdown: string): number[] {
  return markdown.length <= 400 ? [0, 1, 3, 16] : [0, 3, 16];
}

/**
 * The sanitiser's schema plus remark-rehype's footer, which the pipeline does not emit (the renderer draws
 * the notes section itself, notes.tsx): the section, its heading and the back-references' English labels.
 */
const FOOTER_SCHEMA = {
  ...SCHEMA,
  tagNames: [...(SCHEMA.tagNames ?? []), "section"],
  attributes: {
    ...SCHEMA.attributes,
    a: [...(SCHEMA.attributes?.a ?? []), "ariaLabel"],
    section: ["dataFootnotes", ["className", "footnotes"]],
    h2: ["id", ["className", "sr-only"]],
  },
} satisfies typeof SCHEMA;

/**
 * The answer as one block, without the splitter: what rendering block by block must reproduce. Its
 * footnotes section's heading gets the answer's scope, as every generated id does in ours (remark-rehype
 * gives it the fixed id `footnote-label`). Inline tags and heading anchors as the renderer makes them, and
 * the input within nesting.ts's limits, as the renderer's is (so an HTML block of kinds 1 to 5 never
 * starts, in one piece either).
 */
export function unsplit(text: string): string {
  const capped = capNesting(text);
  const hast = toHast(parse(capped), { clobberPrefix: "spec-", handlers: HANDLERS }) as Root;
  markCutTags(hast);
  if (hasStandIns(capped)) restoreText(hast);
  pairTags(hast, false);
  // The answer's headings get anchors, remark-rehype's footer heading does not (the renderer draws its own).
  const body: Root = {
    type: "root",
    children: hast.children.filter(
      (child) => !(child.type === "element" && child.tagName === "section"),
    ),
  };
  const slugs = numberSlugs([anchorHeadings(body, "spec")])[0]!;
  const clean = withSlugs(sanitize(hast, FOOTER_SCHEMA) as Root, slugs, (value) =>
    headingId("user-content-spec", value),
  );
  return toHtml(clean).replaceAll(
    '"user-content-footnote-label"',
    '"user-content-spec-footnote-label"',
  );
}
