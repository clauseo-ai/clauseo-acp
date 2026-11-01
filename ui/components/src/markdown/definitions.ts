// Document-wide link reference definitions. CommonMark lets a reference use a definition from anywhere in
// the document (first definition of a label wins, and definitions inside block quotes and list items count),
// but each block is parsed on its own. So each update gathers every block's own definitions, keeps the
// first of each label, and prepends them, as canonical definition lines and a blank line, to the parse
// input of every block that could hold a reference. Prepending is safe where appending is not: an open
// fence or HTML block at a block's end would swallow appended lines, while the blank line after the
// prepended ones leaves the block to start exactly as it does in the document.
import type { Definition, Nodes, Root } from "mdast";

/** Every definition in `root`, in document order, however deeply nested. */
export function definitionsIn(root: Root): Definition[] {
  const found: Definition[] = [];
  const walk = (node: Nodes) => {
    if (node.type === "definition") found.push(node);
    else if ("children" in node) for (const child of node.children) walk(child);
  };
  walk(root);
  return found;
}

/** A block can define something only if it holds `]:` (a label followed by a colon). */
export function mayDefine(block: string): boolean {
  return block.includes("]:");
}

/** A block can reference something only if it holds `]`. */
export function mayReference(block: string): boolean {
  return block.includes("]");
}

/** A destination in angle brackets: escapes for `\ < > &`, and character references for line endings. */
function destination(url: string): string {
  return `<${url
    .replace(/[\\<>&]/g, "\\$&")
    .replace(/\r/g, "&#13;")
    .replace(/\n/g, "&#10;")}>`;
}

/** A title in double quotes, with the same treatment (a title may not hold a blank line). */
function title(value: string): string {
  return `"${value
    .replace(/[\\"&]/g, "\\$&")
    .replace(/\r/g, "&#13;")
    .replace(/\n/g, "&#10;")}"`;
}

/**
 * One definition as a line that parses back to the same identifier, URL and title. The label is the
 * identifier: micromark keeps the label's source (escapes included) with whitespace collapsed and case
 * folded, which normalises to itself.
 */
export function canonical(definition: Definition): string {
  const titled = definition.title == null ? "" : ` ${title(definition.title)}`;
  return `[${definition.identifier}]: ${destination(definition.url)}${titled}\n`;
}

/**
 * The lines to prepend for a document whose blocks define `perBlock` (each block's own definitions, in
 * order): the first definition of each label, then a blank line; "" when nothing is defined.
 */
export function definitionPrefix(perBlock: readonly (readonly Definition[])[]): string {
  const seen = new Set<string>();
  let lines = "";
  for (const definitions of perBlock) {
    for (const definition of definitions) {
      const key = definition.identifier.toUpperCase();
      if (seen.has(key)) continue;
      seen.add(key);
      lines += canonical(definition);
    }
  }
  return lines === "" ? "" : `${lines}\n`;
}
