// A helper's report as the card's short plain text: the agent's Markdown parsed by the answer's own
// parser (components/markdown/parse.ts) and read back as its words, so "Sale_deed_2004.pdf" keeps
// its underscores, emphasis and headings lose only their marks, and a link reads as its words,
// never its address. Pure.
import { parse } from "@clauseo/components/markdown/parse.ts";

/** The part of an mdast node this reads. */
type Node = { type: string; value?: string; alt?: string | null; children?: Node[] };

/** Kinds read as nothing: raw HTML, a footnote's or a link's definition, an image with no words. */
const SILENT: ReadonlySet<string> = new Set(["html", "definition", "footnoteDefinition"]);

function wordsOf(node: Node, out: string[]): void {
  if (SILENT.has(node.type)) return;
  if (node.type === "text" || node.type === "inlineCode" || node.type === "code") {
    out.push(node.value ?? "");
    return;
  }
  if (node.type === "image" || node.type === "imageReference") {
    if (node.alt) out.push(node.alt);
    return;
  }
  if (node.type === "break") {
    out.push(" ");
    return;
  }
  for (const child of node.children ?? []) wordsOf(child, out);
  // blocks part their words
  if (node.type !== "link" && node.type !== "emphasis" && node.type !== "strong")
    if (node.type !== "delete" && node.type !== "linkReference") out.push(" ");
}

/** The words of `markdown`'s blocks, each block's words one string, in order; whitespace one space.
 */
export function plainBlocks(markdown: string): string[] {
  const root = parse(markdown) as unknown as Node;
  const blocks: string[] = [];
  for (const block of root.children ?? []) {
    const out: string[] = [];
    wordsOf(block, out);
    const line = out.join("").replace(/\s+/g, " ").trim();
    if (line !== "") blocks.push(line);
  }
  return blocks;
}

/** The first block's words: a report's first paragraph as plain text, "" when it has none. */
export const firstPlain = (markdown: string): string => plainBlocks(markdown)[0] ?? "";

/** Every block's words, one after another. */
export const allPlain = (markdown: string): string => plainBlocks(markdown).join(" ");
