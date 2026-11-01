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
import { toHtml } from "hast-util-to-html";
import { createAnswer } from "./answer.ts";

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

function attributes(properties: Properties): string {
  const out: string[] = [];
  for (const key of Object.keys(properties).sort()) {
    if (key === "title") continue;
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
      const open = `<${name}${attributes(node.properties)}>`;
      if (VOID.has(name)) out += open;
      else
        out += `${open}${children(node.children, inPre || name === "pre", BLOCK.has(name))}</${name}>`;
    } else if (node.type === "comment") {
      out += `<!--${node.value}-->`;
    }
  });
  return out;
}

/** The canonical form of an HTML fragment. */
export function normalise(html: string): string {
  const tree: Root = fromHtml(html, { fragment: true });
  return children(tree.children, false, true);
}

/** The HTML of the answer model's final hast: each block's sanitised hast, in order. */
export function htmlOf(views: readonly { hast: Root }[]): string {
  return views.map((view) => toHtml(view.hast)).join("\n");
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
