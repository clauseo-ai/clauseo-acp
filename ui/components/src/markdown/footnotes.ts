// Document-wide footnotes. Each block is parsed on its own, so a block that calls `[^label]` gets a
// placeholder definition of each label it may call (`[^label]: x` and a blank line, before its text), and
// micromark makes footnoteReference nodes there; the placeholders themselves are never shown. The
// references and the real definitions of every block are then numbered for the whole answer, as GFM
// numbers one document: in order of first reference, repeats keep their number and each gets its own
// back-reference. As cmark-gfm (GitHub) does, the references are met in document order, those inside a
// note where the note stands, so a note's own references can take a number before later text's. A
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

const DEFINES = /\[\^((?:[^\s[\]\\]|\\[^\s])+)\]:/g;
const OWN = /^[ \t]*(?:>[ \t]*)*\[\^((?:[^\s[\]\\]|\\[^\s])+)\]:/gm;

/**
 * What `text` may define, as identifiers: `defines`, each `[^label]:` anywhere (more than its notes,
 * never fewer); `own`, each at the start of a line after quote markers, the notes it most likely holds
 * itself (checked against its parse before a placeholder is left out for one); `links`, whether a `]:`
 * is left once those are taken out, so the text may hold a link definition.
 */
export function definedIn(text: string): {
  defines: ReadonlySet<string>;
  own: ReadonlySet<string>;
  links: boolean;
} {
  const defines = new Set<string>();
  const own = new Set<string>();
  if (!text.includes("]:")) return { defines, own, links: false };
  for (const match of text.matchAll(DEFINES))
    if (match[1]!.length <= MAX_LABEL) defines.add(identifierOf(match[1]!));
  for (const match of text.matchAll(OWN))
    if (match[1]!.length <= MAX_LABEL) own.add(identifierOf(match[1]!));
  return { defines, own, links: text.replace(DEFINES, "").includes("]:") };
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

/**
 * `root` without the block quote of placeholders its input may start with. `from` is where the block's own
 * text starts in the input: only a quote that ends before it is the placeholders' (a block of the agent's
 * that starts with `> [^` is a quote of its own, and stays).
 */
export function withoutPlaceholders(root: Root, from: number): Root {
  const first = root.children[0];
  if (from === 0 || first?.type !== "blockquote" || (first.position?.end.offset ?? from + 1) > from)
    return root;
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

/**
 * What a block's parse says about footnotes: its references outside notes, in order; its notes; and both
 * in document order (a reference's identifier, or a note where it stands).
 */
export type Footnotes = {
  readonly refs: readonly string[];
  readonly notes: readonly Note[];
  readonly order: readonly (string | Note)[];
};

export const NO_FOOTNOTES: Footnotes = { refs: [], notes: [], order: [] };

/**
 * The footnote references and real definitions of `root`, parsed from `input`, whose block text starts at
 * `from` (the placeholders before it are not the block's). An iterative walk: the tree may be deep.
 */
export function footnotesIn(root: Root, input: string, from: number): Footnotes {
  const refs: string[] = [];
  const found: FootnoteDefinition[] = [];
  const events: (string | FootnoteDefinition)[] = [];
  const definitions: Definition[] = [];
  const collect = (start: Nodes, into: string[], notes: boolean) => {
    const stack: Nodes[] = [start];
    for (let node = stack.pop(); node; node = stack.pop()) {
      if (node.type === "footnoteReference") {
        into.push(node.identifier);
        if (notes) events.push(node.identifier);
      } else if (node.type === "definition") definitions.push(node);
      else if (node.type === "footnoteDefinition" && node !== start) {
        if (notes && (node.position?.start.offset ?? 0) >= from) {
          found.push(node);
          events.push(node);
        }
      } else if ("children" in node)
        for (let index = node.children.length - 1; index >= 0; index--)
          stack.push(node.children[index]!);
    }
  };
  collect(root, refs, true);
  if (refs.length === 0 && found.length === 0) return NO_FOOTNOTES;
  const byNode = new Map<FootnoteDefinition, Note>();
  const notes = found.map((node) => {
    const inner: string[] = [];
    collect(node, inner, false);
    const source = input.slice(node.position?.start.offset ?? 0, node.position?.end.offset ?? 0);
    const note = { id: node.identifier, node, refs: inner, definitions, source };
    byNode.set(node, note);
    return note;
  });
  const order = events.map((event) => (typeof event === "string" ? event : byNode.get(event)!));
  return { refs, notes, order };
}

/** The answer's footnote numbering for one update. */
export type Numbering = {
  /** Number of each counted identifier. */
  readonly number: ReadonlyMap<string, number>;
  /** The first definition of each identifier. */
  readonly notes: ReadonlyMap<string, Note>;
  /** References to each identifier in the whole answer, notes included. */
  readonly total: ReadonlyMap<string, number>;
  /** Per block: the occurrence number (nth) of each of its references, per identifier; its view signature. */
  readonly blocks: readonly { nths: Nths; signature: string }[];
  /** Per shown note: the occurrence numbers of the references inside it. */
  readonly inNotes: ReadonlyMap<string, Nths>;
  readonly order: readonly string[];
};

/** Per identifier, the occurrence number of each of its references in one block or note, in order. */
export type Nths = ReadonlyMap<string, readonly number[]>;

/**
 * Number the references of `perBlock` (each block's footnotes) for the whole answer. A reference counts
 * when its label is defined, or always while streaming (it shows as a pending number). A note is shown
 * when a counted reference outside notes, or inside a shown note, calls it; only a shown note's own
 * references count, so no back-reference points into a note that is not shown (cmark-gfm counts those
 * too, and its back-reference then leads nowhere). Then every counted reference is met in document order,
 * a shown note's where the note stands, as cmark-gfm meets them.
 */
export function numberFootnotes(perBlock: readonly Footnotes[], streaming: boolean): Numbering {
  const notes = new Map<string, Note>();
  for (const footnotes of perBlock)
    for (const note of footnotes.notes) if (!notes.has(note.id)) notes.set(note.id, note);
  const counts = (id: string) => streaming || notes.has(id);
  const shown = new Set<Note>();
  const queue: string[] = [];
  for (const footnotes of perBlock) for (const id of footnotes.refs) if (counts(id)) queue.push(id);
  for (let id = queue.pop(); id !== undefined; id = queue.pop()) {
    const note = notes.get(id);
    if (!note || shown.has(note)) continue;
    shown.add(note);
    for (const inner of note.refs) if (counts(inner)) queue.push(inner);
  }
  const number = new Map<string, number>();
  const total = new Map<string, number>();
  const order: string[] = [];
  const count = (id: string, into: Map<string, number[]>) => {
    if (!number.has(id)) {
      order.push(id);
      number.set(id, order.length);
    }
    const nth = (total.get(id) ?? 0) + 1;
    total.set(id, nth);
    const list = into.get(id);
    if (list) list.push(nth);
    else into.set(id, [nth]);
  };
  const inNotes = new Map<string, Nths>();
  const blocks = perBlock.map((footnotes) => {
    if (footnotes.order.length === 0) return { nths: EMPTY, signature: "" };
    const nths = new Map<string, number[]>();
    for (const event of footnotes.order) {
      if (typeof event === "string") {
        if (counts(event)) count(event, nths);
      } else if (shown.has(event)) {
        const inner = new Map<string, number[]>();
        for (const id of event.refs) if (counts(id)) count(id, inner);
        inNotes.set(event.id, inner);
      }
    }
    let signature = "";
    for (const [id, list] of nths)
      signature += `${id}\u0000${number.get(id)},${list.join(".")},${notes.has(id) ? 1 : 0}\u0001`;
    return { nths: nths.size === 0 ? EMPTY : nths, signature };
  });
  return { number, notes, total, blocks, inNotes, order };
}

const EMPTY: Nths = new Map();

/** A fragment for an id: its characters percent-encoded where a URL needs it. */
const fragment = (id: string) => encodeURIComponent(id);

/**
 * The id part of the `nth` reference to note `id`: the identifier, then `-n` for a repeat, as
 * remark-rehype and cmark-gfm write it, but with each hyphen of the identifier doubled, so the second
 * reference to `a` (`a-2`) and the first to `a-2` (`a--2`) never share an id.
 */
function refPart(id: string, nth: number): string {
  return `${id.replaceAll("-", "--")}${nth > 1 ? `-${nth}` : ""}`;
}

/**
 * The footnoteReference handler for one block (or note): the reference's answer-wide number, its own
 * back-reference id for each repeat, a link to the note when it is defined, and a pending number when not.
 */
export function referenceHandler(scope: string, numbering: Numbering, nths: Nths): Handler {
  const met = new Map<string, number>();
  return (_state, node: FootnoteReference) => {
    const id = node.identifier;
    const number = numbering.number.get(id);
    if (number === undefined) {
      const text: Text = { type: "text", value: `[^${node.label ?? id}]` };
      return text;
    }
    const index = met.get(id) ?? 0;
    met.set(id, index + 1);
    const nth = nths.get(id)?.[index] ?? index + 1;
    const value: Text = { type: "text", value: String(number) };
    if (!numbering.notes.has(id)) {
      return {
        type: "element",
        tagName: "sup",
        // Its spoken label is the superscript component's (notes.tsx).
        properties: { dataFootnotePending: String(number) },
        children: [value],
      };
    }
    const link: Element = {
      type: "element",
      tagName: "a",
      properties: {
        href: `#${scope}-fn-${fragment(id)}`,
        id: `${scope}-fnref-${refPart(id, nth)}`,
        dataFootnoteRef: true,
        ariaDescribedBy: [`${scope}-footnote-label`],
      },
      children: [value],
    };
    return { type: "element", tagName: "sup", properties: {}, children: [link] };
  };
}

/**
 * The back-references of a note: one per reference, as remark-rehype writes them, except that each
 * carries its label's number (`1`, `1-2`) rather than the English label.
 */
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
        href: `#${scope}-fnref-${fragment(refPart(id, nth))}`,
        // The back-reference's label, "Back to reference {n}", is the link component's (words.ts).
        dataFootnoteBackref: `${index + 1}${nth > 1 ? `-${nth}` : ""}`,
        className: ["data-footnote-backref"],
      },
      children,
    });
  }
  return links;
}

/**
 * One note of the notes section, unsanitised: a root holding its list item, its content rendered with
 * `options` (and the reference handler for the references inside it), then its back-references. `id` is
 * a shown note's identifier; each note is its own view, so one that changes re-renders alone. The
 * section around the items (notes.tsx) is the same for every answer.
 */
export function noteHast(scope: string, numbering: Numbering, id: string, options: Options): Hast {
  const note = numbering.notes.get(id)!;
  const index = numbering.number.get(id)! - 1;
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
  // The space before them is a text node of its own: added to the note's last text, it made a closing
  // tag there (`[^n]: A <script>y</script>`) no tag to inline-tags.ts, which matches a tag's whole text.
  if (last?.type === "element" && last.tagName === "p")
    last.children.push({ type: "text", value: " " }, ...back);
  else content.push(...back);
  const item: Element = {
    type: "element",
    tagName: "li",
    properties: { id: `${scope}-fn-${id}` },
    children: content,
  };
  return { type: "root", children: [item] };
}

/**
 * Leaves out of a note's hast (noteHast) each back-reference to a reference in `lost` (by its id, as
 * referenceHandler sets it): a reference a dropped never-markup pair shows as plain text
 * (inline-tags.ts), so no anchor is there to go back to. The space before it goes with it.
 */
export function dropBackReferences(root: Hast, lost: ReadonlySet<string>): void {
  const stack: (Hast | Element)[] = [root];
  for (let parent = stack.pop(); parent; parent = stack.pop()) {
    const children = parent.children as ElementContent[];
    for (let index = children.length - 1; index >= 0; index--) {
      const child = children[index]!;
      if (child.type !== "element") continue;
      const href = child.properties["href"];
      if (!child.properties["dataFootnoteBackref"] || typeof href !== "string") {
        stack.push(child);
        continue;
      }
      let target = "";
      try {
        target = decodeURIComponent(href.slice(1));
      } catch {
        continue;
      }
      if (!lost.has(target)) continue;
      const before = children[index - 1];
      const space = before?.type === "text" && before.value === " " ? 1 : 0;
      children.splice(index - space, 1 + space);
      index -= space;
    }
  }
}
