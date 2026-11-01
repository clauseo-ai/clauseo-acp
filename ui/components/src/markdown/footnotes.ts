// Document-wide footnotes. Each block is parsed on its own, so a block that calls `[^label]` gets a
// placeholder definition of each label it may call (`[^label]: x` and a blank line, before its text), and
// micromark makes footnoteReference nodes there; the placeholders themselves are never shown. The
// references and the real definitions of every block are then numbered for the whole answer, as GFM
// numbers one document: in order of first reference, repeats keep their number and each gets its own
// back-reference, and references inside notes follow the ones in the text (GitHub's footer loop). A
// block's view depends only on its references' numbers and states, so a note that arrives re-renders
// just the blocks that call it. The notes render once, after the last block, as remark-rehype's footer
// would render them for the whole answer, with every id carrying the answer's scope.
//
// While streaming, a reference whose note has not arrived is a pending number (a <sup>, no link) and a
// note still streaming in the last block already counts. At the end, a block's placeholders are only
// those of defined labels, so an undefined reference is literal text, as in GFM.
import type { Element, ElementContent, Root as Hast, Text } from "hast";
import type { Definition, FootnoteDefinition, FootnoteReference, Nodes, Root } from "mdast";
import { toHast } from "mdast-util-to-hast";
import type { Handler, Options } from "mdast-util-to-hast";

/** A footnote label as written: no whitespace, no unescaped bracket; a backslash escapes the next character. */
const CALL = /\[\^((?:[^\s[\]\\]|\\[^\s])+)\]/g;
const MAX_LABEL = 999;

/** The labels (as written, once each) of every `[^label]` in `text`: what a block may call or define. */
export function footnoteLabelsIn(text: string): readonly string[] {
  if (!text.includes("[^")) return [];
  const found = new Set<string>();
  for (const match of text.matchAll(CALL)) {
    const label = match[1]!;
    if (label.length <= MAX_LABEL) found.add(label);
  }
  return [...found];
}

/** The identifier micromark gives a label (normalised, case folded, in lower case). */
export function identifierOf(label: string): string {
  return label.toLowerCase().toUpperCase().toLowerCase();
}

/**
 * The placeholder definitions for `labels`, in one block quote and then a blank line; "" for none. The
 * quote ends at the blank line, so the block starts after it as it does in the document: a definition's
 * own content would carry on into an indented line after a blank line, a quote's does not. The quote is
 * dropped before rendering (withoutPlaceholders).
 */
export function placeholdersFor(labels: readonly string[]): string {
  if (labels.length === 0) return "";
  let prefix = "";
  for (const label of labels) prefix += `> [^${label}]: x\n`;
  return `${prefix}\n`;
}

/** `root`, parsed from `input`, without the block quote of placeholders it may start with. */
export function withoutPlaceholders(root: Root, input: string): Root {
  if (!input.startsWith("> [^") || root.children[0]?.type !== "blockquote") return root;
  return { ...root, children: root.children.slice(1) };
}

/** A real footnote definition of a block, with the references inside it, in order. */
export type Note = {
  readonly id: string;
  readonly node: FootnoteDefinition;
  readonly refs: readonly string[];
  /** The link definitions of the block it is in, so its link references resolve. */
  readonly definitions: readonly Definition[];
  /** Its text, as parsed. */
  readonly source: string;
};

/** What a block's parse says about footnotes: its references outside notes, in order, and its notes. */
export type Footnotes = { readonly refs: readonly string[]; readonly notes: readonly Note[] };

export const NO_FOOTNOTES: Footnotes = { refs: [], notes: [] };

/**
 * The footnote references and real definitions of `root`, parsed from `input`, whose block text starts at
 * `from` (the placeholders before it are not the block's). An iterative walk: the tree may be deep.
 */
export function footnotesIn(root: Root, input: string, from: number): Footnotes {
  const refs: string[] = [];
  const found: FootnoteDefinition[] = [];
  const definitions: Definition[] = [];
  const collect = (start: Nodes, into: string[], notes: boolean) => {
    const stack: Nodes[] = [start];
    for (let node = stack.pop(); node; node = stack.pop()) {
      if (node.type === "footnoteReference") into.push(node.identifier);
      else if (node.type === "definition") definitions.push(node);
      else if (node.type === "footnoteDefinition" && node !== start) {
        if (notes && (node.position?.start.offset ?? 0) >= from) found.push(node);
      } else if ("children" in node)
        for (let index = node.children.length - 1; index >= 0; index--)
          stack.push(node.children[index]!);
    }
  };
  collect(root, refs, true);
  if (refs.length === 0 && found.length === 0) return NO_FOOTNOTES;
  const notes = found.map((node) => {
    const inner: string[] = [];
    collect(node, inner, false);
    const source = input.slice(node.position?.start.offset ?? 0, node.position?.end.offset ?? 0);
    return { id: node.identifier, node, refs: inner, definitions, source };
  });
  return { refs, notes };
}

/** The answer's footnote numbering for one update. */
export type Numbering = {
  /** Number of each counted identifier. */
  readonly number: ReadonlyMap<string, number>;
  /** The first definition of each identifier. */
  readonly notes: ReadonlyMap<string, Note>;
  /** References to each identifier in the whole answer, notes included. */
  readonly total: ReadonlyMap<string, number>;
  /** Per block: references to each of its identifiers before the block; and its view signature. */
  readonly blocks: readonly { before: ReadonlyMap<string, number>; signature: string }[];
  /** Per note, in number order: references before it, for the references inside it. */
  readonly inNotes: ReadonlyMap<string, ReadonlyMap<string, number>>;
  readonly order: readonly string[];
};

/**
 * Number the references of `perBlock` (each block's footnotes, in order) for the whole answer. A reference
 * counts when its label is defined, or always while streaming (it shows as a pending number).
 */
export function numberFootnotes(perBlock: readonly Footnotes[], streaming: boolean): Numbering {
  const notes = new Map<string, Note>();
  for (const footnotes of perBlock)
    for (const note of footnotes.notes) if (!notes.has(note.id)) notes.set(note.id, note);
  const number = new Map<string, number>();
  const total = new Map<string, number>();
  const order: string[] = [];
  const count = (id: string) => {
    if (!streaming && !notes.has(id)) return;
    if (!number.has(id)) {
      order.push(id);
      number.set(id, order.length);
    }
    total.set(id, (total.get(id) ?? 0) + 1);
  };
  const blocks = perBlock.map((footnotes) => {
    if (footnotes.refs.length === 0) return { before: EMPTY, signature: "" };
    const before = new Map<string, number>();
    for (const id of footnotes.refs)
      if (!before.has(id) && (streaming || notes.has(id))) before.set(id, total.get(id) ?? 0);
    for (const id of footnotes.refs) count(id);
    let signature = "";
    for (const [id, seen] of before)
      signature += `${id}\u0000${number.get(id)},${seen},${notes.has(id) ? 1 : 0}\u0001`;
    return { before, signature };
  });
  const inNotes = new Map<string, Map<string, number>>();
  for (let index = 0; index < order.length; index++) {
    const note = notes.get(order[index]!);
    if (!note) continue;
    const before = new Map<string, number>();
    for (const id of note.refs) if (!before.has(id)) before.set(id, total.get(id) ?? 0);
    inNotes.set(note.id, before);
    for (const id of note.refs) count(id);
  }
  return { number, notes, total, blocks, inNotes, order };
}

const EMPTY: ReadonlyMap<string, number> = new Map();

/** A fragment for an id: its characters percent-encoded where a URL needs it. */
const fragment = (id: string) => encodeURIComponent(id);

/**
 * The footnoteReference handler for one block (or note): the reference's answer-wide number, its own
 * back-reference id for each repeat, a link to the note when it is defined, and a pending number when not.
 */
export function referenceHandler(
  scope: string,
  numbering: Numbering,
  before: ReadonlyMap<string, number>,
): Handler {
  const seen = new Map(before);
  return (_state, node: FootnoteReference) => {
    const id = node.identifier;
    const number = numbering.number.get(id);
    if (number === undefined) {
      const text: Text = { type: "text", value: `[^${node.label ?? id}]` };
      return text;
    }
    const nth = (seen.get(id) ?? 0) + 1;
    seen.set(id, nth);
    const value: Text = { type: "text", value: String(number) };
    if (!numbering.notes.has(id)) {
      return {
        type: "element",
        tagName: "sup",
        properties: { dataFootnotePending: true },
        children: [value],
      };
    }
    const link: Element = {
      type: "element",
      tagName: "a",
      properties: {
        href: `#${scope}-fn-${fragment(id)}`,
        id: `${scope}-fnref-${id}${nth > 1 ? `-${nth}` : ""}`,
        dataFootnoteRef: true,
        ariaDescribedBy: [`${scope}-footnote-label`],
      },
      children: [value],
    };
    return { type: "element", tagName: "sup", properties: {}, children: [link] };
  };
}

/** The back-references of a note: one per reference, as remark-rehype writes them. */
function backReferences(scope: string, id: string, index: number, count: number): ElementContent[] {
  const links: ElementContent[] = [];
  for (let nth = 1; nth <= count; nth++) {
    if (links.length > 0) links.push({ type: "text", value: " " });
    const children: ElementContent[] = [{ type: "text", value: "↩" }];
    if (nth > 1)
      children.push({
        type: "element",
        tagName: "sup",
        properties: {},
        children: [{ type: "text", value: String(nth) }],
      });
    links.push({
      type: "element",
      tagName: "a",
      properties: {
        href: `#${scope}-fnref-${fragment(id)}${nth > 1 ? `-${nth}` : ""}`,
        dataFootnoteBackref: "",
        ariaLabel: `Back to reference ${index + 1}${nth > 1 ? `-${nth}` : ""}`,
        className: ["data-footnote-backref"],
      },
      children,
    });
  }
  return links;
}

/**
 * The notes section for the whole answer, unsanitised: each referenced note in number order, its content
 * rendered with `options` (and the reference handler for references inside it), then its back-references.
 * Undefined when no defined note is referenced.
 */
export function notesHast(scope: string, numbering: Numbering, options: Options): Hast | undefined {
  const items: ElementContent[] = [];
  numbering.order.forEach((id, index) => {
    const note = numbering.notes.get(id);
    if (!note) return;
    const handler = referenceHandler(scope, numbering, numbering.inNotes.get(id) ?? EMPTY);
    const tree: Root = { type: "root", children: [...note.definitions, ...note.node.children] };
    const content = (
      toHast(tree, {
        ...options,
        handlers: { ...options.handlers, footnoteReference: handler },
      }) as Hast
    ).children.filter((child): child is ElementContent => child.type !== "doctype");
    const back = backReferences(scope, id, index, numbering.total.get(id) ?? 0);
    let last: ElementContent | undefined;
    for (const child of content) if (child.type === "element") last = child;
    if (last?.type === "element" && last.tagName === "p") {
      const end = last.children.at(-1);
      if (end?.type === "text") end.value += " ";
      else last.children.push({ type: "text", value: " " });
      last.children.push(...back);
    } else content.push(...back);
    items.push({ type: "text", value: "\n" });
    items.push({
      type: "element",
      tagName: "li",
      properties: { id: `${scope}-fn-${id}` },
      children: content,
    });
  });
  if (items.length === 0) return undefined;
  items.push({ type: "text", value: "\n" });
  const section: Element = {
    type: "element",
    tagName: "section",
    properties: { dataFootnotes: true, className: ["footnotes"] },
    children: [
      {
        type: "element",
        tagName: "h2",
        properties: { className: ["sr-only"], id: `${scope}-footnote-label` },
        children: [{ type: "text", value: "Footnotes" }],
      },
      { type: "text", value: "\n" },
      { type: "element", tagName: "ol", properties: {}, children: items },
      { type: "text", value: "\n" },
    ],
  };
  return { type: "root", children: [section] };
}
