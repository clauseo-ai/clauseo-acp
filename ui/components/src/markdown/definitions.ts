// Document-wide link reference definitions. CommonMark lets a reference use a definition from anywhere in
// the document (first definition of a label wins, and definitions inside block quotes and list items count),
// but each block is parsed on its own. So each update gathers every block's own definitions, keeps the
// first of each label, and prepends to each block's parse input, as canonical definition lines and a blank
// line, only the definitions whose labels that block uses. A block's input, and so its view, then changes
// only when a definition it uses arrives or changes: a definition arriving at the end of a long answer
// re-renders the blocks that cite it, not every block with a bracket. Prepending is safe where appending is
// not: an open fence or HTML block at a block's end would swallow appended lines, while the blank line
// after the prepended ones leaves the block to start exactly as it does in the document.
import type { Definition, Nodes, Root } from "mdast";

/** One definition as the canonical line to prepend, under its label's key (keyOf). */
export type DefinitionLine = { readonly key: string; readonly line: string };
/** The document's definitions: the canonical line of the first definition of each key. */
export type DefinitionLines = ReadonlyMap<string, string>;

/**
 * The key micromark matches a reference to a definition by (micromark-util-normalize-identifier, which is
 * not a direct dependency): whitespace collapsed and trimmed, then case folded. mdast's `identifier` is
 * this in lower case.
 */
export function keyOf(label: string): string {
  return label
    .replace(/[\t\n\r ]+/g, " ")
    .replace(/^ | $/g, "")
    .toLowerCase()
    .toUpperCase();
}

/** A block can define something only if it holds `]:` (a label followed by a colon). */
export function mayDefine(block: string): boolean {
  return block.includes("]:");
}

/** A block can reference something only if it holds `]`. */
export function mayReference(block: string): boolean {
  return block.includes("]");
}

/** A label may hold at most 999 characters (CommonMark 4.7). */
const MAX_LABEL = 999;

/**
 * The keys (keyOf) of every innermost `[…]` pair in `text` (where every label is, since a label holds no
 * unescaped bracket), once honouring backslash escapes and once not: a backslash inside a code span is no
 * escape, and over-collecting only costs a lookup. A label over several lines in a block quote holds the
 * quote's markers, so each is also taken with what starts its continuation lines dropped.
 */
export function labelsIn(text: string): readonly string[] {
  const found = new Set<string>();
  const add = (label: string) => {
    if (label.length > MAX_LABEL) return;
    found.add(keyOf(label));
    if (label.includes("\n")) found.add(keyOf(label.replace(/\n[ \t>]*/g, " ")));
  };
  for (const escapes of [true, false]) {
    let open = -1;
    for (let at = 0; at < text.length; at++) {
      const char = text.charCodeAt(at);
      if (char === 92 /* \ */ && escapes) at++;
      else if (char === 91 /* [ */) open = at;
      else if (char === 93 /* ] */ && open >= 0) {
        add(text.slice(open + 1, at));
        open = -1;
      }
    }
  }
  return [...found];
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
 * The label to write for `definition`, parsed from `source`: its source label (escapes and character
 * references as written; mdast's `label` is decoded), which normalises to the identifier and is never
 * longer than 999 characters. The identifier itself can be longer, since case folding can grow a label
 * (`ß` becomes `ss`). Falls back to the identifier if the source label cannot be found.
 */
function labelOf(definition: Definition, source: string): string {
  const start = definition.position?.start.offset;
  if (start !== undefined && source[start] === "[") {
    for (let at = start + 1; at < source.length && at - start <= MAX_LABEL + 1; at++) {
      const char = source[at];
      if (char === "\\") at++;
      else if (char === "[") break;
      else if (char === "]") {
        const label = source.slice(start + 1, at).replace(/\n[ \t>]*/g, " ");
        if (keyOf(label).toLowerCase() === definition.identifier) return label;
        break;
      }
    }
  }
  return definition.identifier;
}

/** One definition as a line that parses back to the same identifier, URL and title, and its key. */
export function canonical(definition: Definition, source: string): DefinitionLine {
  const label = labelOf(definition, source);
  const titled = definition.title == null ? "" : ` ${title(definition.title)}`;
  return { key: keyOf(label), line: `[${label}]: ${destination(definition.url)}${titled}\n` };
}

/** Every definition in `root` (parsed from `source`), in document order, however deeply nested. */
export function definitionsIn(root: Root, source: string): DefinitionLine[] {
  const found: DefinitionLine[] = [];
  const walk = (node: Nodes) => {
    if (node.type === "definition") found.push(canonical(node, source));
    else if ("children" in node) for (const child of node.children) walk(child);
  };
  walk(root);
  return found;
}

/** The document's definitions, from each block's own (in order): the first of each key wins. */
export function definitionLines(perBlock: readonly (readonly DefinitionLine[])[]): DefinitionLines {
  const lines = new Map<string, string>();
  for (const definitions of perBlock) {
    for (const { key, line } of definitions) if (!lines.has(key)) lines.set(key, line);
  }
  return lines;
}

/**
 * What to prepend to a block whose labels have the keys `labels` (labelsIn): the lines of the defined ones, then a blank
 * line; "" when it uses none.
 */
export function prefixFor(labels: readonly string[], lines: DefinitionLines): string {
  let prefix = "";
  for (const label of labels) prefix += lines.get(label) ?? "";
  return prefix === "" ? "" : `${prefix}\n`;
}
