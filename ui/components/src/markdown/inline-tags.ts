// The approved inline tags (issue #137 §4): sup, sub, u, ins, del, s, mark, br, small and kbd, nothing else.
// Raw HTML is never parsed as HTML. micromark already reads each inline tag as its own html node, which the
// mdast-to-hast handler here turns into a text node (React escapes text). Then, in each element's children,
// an allowlisted opening tag and its closing tag become one of our elements, with no properties: every
// attribute is dropped, whatever it was (`style`, `class`, `on…`). Everything else stays the text it was
// written as, so a lawyer sees every character the agent wrote:
// - Any tag not on the list (`<script>`, `<img>`, `<a>`, `<div>`, and placeholders like `<Party A>` or
//   `<insert date>`), comments, and tags written as entities (`&lt;sup&gt;`, which are text already).
// - An opening tag with no closing tag in the same parent (a tag cannot pair across a paragraph, a list item
//   or an emphasis it is not inside, nor across a block element or a new paragraph among its siblings, as
//   a tight list item or a definition has), a stray closing tag, a self-closed `<sup/>`, `</br>`, and of
//   misnested tags (`<sup><sub>a</sup></sub>`) the inner one.
// - Past MAX_TAG_NESTING allowlisted elements inside each other, or MAX_DEPTH levels of hast, the deeper
//   tags (hostile nesting: the tree is walked recursively later).
// `<br>`, `<br/>` and `<br />` are void: a line break. A block of HTML holding only those (`<br>` on a line
// of its own) is line breaks too; any other HTML block stays text.
//
// Tag names match ASCII case-insensitively (`<SUP>`); micromark only reads ASCII letters as a tag name, so a
// lookalike (`<ѕup>`, Cyrillic) is text before it gets here.
//
// While streaming, in the last block, an allowlisted opening tag still waiting for its closing tag is
// hidden, and what follows it shows unstyled until the closing tag arrives (repair.ts hides a half-arrived
// `<su` at the very end). Only where a closing tag may still arrive: the children of the elements along the
// end of the block. Anywhere else such a tag can never pair and shows as text, as it does finished.
import type { Element, ElementContent, Root, RootContent, Text } from "hast";
import { defaultHandlers } from "mdast-util-to-hast";
import type { Handler } from "mdast-util-to-hast";

/** The approved inline tags. */
export const ALLOWED_TAGS: ReadonlySet<string> = new Set([
  "sup",
  "sub",
  "u",
  "ins",
  "del",
  "s",
  "mark",
  "br",
  "small",
  "kbd",
]);

/** Allowlisted elements one parent may hold inside each other; deeper tags stay text. */
export const MAX_TAG_NESTING = 32;
/** Levels of hast below a block's root that a paired tag may reach (nesting.ts's MAX_TREE_DEPTH, plus 32). */
const MAX_DEPTH = 160;

/** Text nodes made from inline HTML, and from HTML blocks, of the trees being built. */
const INLINE_HTML = new WeakSet<Text>();
const BLOCK_HTML = new WeakSet<Text>();

/** mdast parents whose html children are HTML blocks; any other parent holds inline HTML. */
const FLOW = new Set([
  "root",
  "blockquote",
  "listItem",
  "footnoteDefinition",
  "defListDescription",
]);

/** Raw HTML stays text: an HTML node becomes a text node, which React escapes (no rehype-raw anywhere). */
export const htmlAsText: Handler = (state, node: { value: string }, parent) => {
  const text: Text = { type: "text", value: node.value };
  state.patch(node as never, text);
  (parent && FLOW.has(parent.type) ? BLOCK_HTML : INLINE_HTML).add(text);
  return text;
};

/**
 * The first hast node of each paragraph. A tight list item and a definition unwrap their paragraphs, so
 * one paragraph's inline content follows another's among the same children: a tag never pairs across that.
 */
const PARAGRAPH_START = new WeakSet<ElementContent>();

/** mdast-util-to-hast's paragraph, noting its first node. */
export const paragraphNotingStart: Handler = (state, node) => {
  const paragraph = defaultHandlers.paragraph(state, node);
  const first = paragraph.children[0];
  if (first) PARAGRAPH_START.add(first);
  return paragraph;
};

/** Block elements: a tag never pairs across one among its siblings (a tight list item's heading, say). */
const BLOCKS = new Set([
  "p",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "pre",
  "ul",
  "ol",
  "dl",
  "blockquote",
  "table",
  "hr",
]);

/** Whether a tag before `node` can never pair with one after it: a block sibling, or a new paragraph. */
function isBarrier(node: ElementContent): boolean {
  if (node.type === "element") return BLOCKS.has(node.tagName);
  return PARAGRAPH_START.has(node) || (node.type === "text" && BLOCK_HTML.has(node));
}

type Tag = { name: string; close: boolean; selfClosed: boolean };

const TAG_NAME = /^<(\/?)([A-Za-z][A-Za-z0-9-]*)/;

/** The allowlisted tag an inline html node is (micromark checked its syntax), or undefined. */
function tagOf(value: string): Tag | undefined {
  const match = TAG_NAME.exec(value);
  if (!match) return undefined;
  const name = match[2]!.toLowerCase();
  if (!ALLOWED_TAGS.has(name)) return undefined;
  return { name, close: match[1] === "/", selfClosed: value.endsWith("/>") };
}

/** An HTML block of nothing but line breaks, each without attributes. */
const BREAKS_ONLY = /^(?:[ \t\n]*<br[ \t]*\/?>)+[ \t\n]*$/i;
const BREAK = /<br/gi;

function element(tagName: string, children: ElementContent[] = []): Element {
  return { type: "element", tagName, properties: {}, children };
}

/**
 * The allowlisted pairs among `nodes`: for each opening tag that pairs, the index of its closing tag. A
 * closing tag pairs with the nearest open tag of its name; tags of other names still open inside that pair
 * are left unpaired for good (`closed`: no closing tag can reach them any more), and so is every tag still
 * open at a barrier (isBarrier). One stack per name keeps this linear however the tags are nested.
 */
function pairsIn(
  tags: readonly (Tag | undefined)[],
  barriers: readonly boolean[],
): {
  pairs: Map<number, number>;
  closed: Set<number>;
} {
  const pairs = new Map<number, number>();
  const closed = new Set<number>();
  const open = new Map<string, number[]>();
  for (let index = 0; index < tags.length; index++) {
    if (barriers[index])
      for (const stack of open.values()) while (stack.length > 0) closed.add(stack.pop()!);
    const tag = tags[index];
    if (!tag || tag.name === "br" || tag.selfClosed) continue;
    if (!tag.close) {
      let stack = open.get(tag.name);
      if (!stack) open.set(tag.name, (stack = []));
      stack.push(index);
      continue;
    }
    const start = open.get(tag.name)?.pop();
    if (start === undefined) continue;
    for (const stack of open.values())
      while ((stack.at(-1) ?? -1) > start) closed.add(stack.pop()!);
    pairs.set(start, index);
  }
  return { pairs, closed };
}

type Frame = { tag: string; children: ElementContent[]; closeAt: number };

/**
 * `nodes` with each allowlisted pair made our element, every `<br>` a line break, and the rest as text;
 * with `hide`, an opening tag that does not pair is left out. `room` is how deep the pairs may nest.
 */
function paired(nodes: ElementContent[], hide: boolean, room: number): ElementContent[] {
  // Each tag is decided here once: what stays text is not paired again inside the elements made here.
  const tags = nodes.map((node) =>
    node.type === "text" && INLINE_HTML.delete(node) ? tagOf(node.value) : undefined,
  );
  const barriers = nodes.map((node, index) => index > 0 && isBarrier(node));
  const { pairs, closed } = pairsIn(tags, barriers);
  const root: Frame = { tag: "", children: [], closeAt: -1 };
  const frames: Frame[] = [root];
  for (let index = 0; index < nodes.length; index++) {
    const node = nodes[index]!;
    const tag = tags[index];
    const frame = frames.at(-1)!;
    if (!tag) {
      frame.children.push(node);
      continue;
    }
    if (tag.name === "br" && !tag.close) {
      frame.children.push(element("br"));
      continue;
    }
    if (index === frame.closeAt) {
      frames.pop();
      frames.at(-1)!.children.push(element(frame.tag, frame.children));
      continue;
    }
    const closeAt = pairs.get(index);
    if (closeAt !== undefined && frames.length <= Math.min(MAX_TAG_NESTING, room)) {
      frames.push({ tag: tag.name, children: [], closeAt });
      continue;
    }
    // Text: a closing tag with no partner, a self-closed tag, an opening tag too deep (its partner is then
    // text too) or with no partner, which is hidden instead while its partner may still arrive.
    const waiting = !tag.close && !tag.selfClosed && closeAt === undefined && !closed.has(index);
    if (!hide || !waiting) frame.children.push(node);
  }
  return root.children;
}

/** The elements along the end of `root`: the last element child of each, down from the root. */
function endPath(root: Root): Set<Root | Element> {
  const path = new Set<Root | Element>([root]);
  let node: Root | Element = root;
  for (;;) {
    let last: RootContent | undefined;
    for (let index = node.children.length - 1; index >= 0; index--) {
      const child: RootContent = node.children[index]!;
      if (child.type === "text" && /^\s*$/.test(child.value)) continue;
      last = child;
      break;
    }
    if (last?.type !== "element") return path;
    path.add(last);
    node = last;
  }
}

/**
 * Makes the allowlisted tags of `root` (built with htmlAsText) our elements, in place. `streaming` is true
 * for the streaming last block, whose unpaired opening tags at its end are hidden.
 */
export function pairTags(root: Root, streaming: boolean): void {
  const open = streaming ? endPath(root) : undefined;
  const stack: [Root | Element, number][] = [[root, 0]];
  for (let item = stack.pop(); item; item = stack.pop()) {
    const [parent, depth] = item;
    let found = false;
    const children = parent.children as ElementContent[];
    for (let index = 0; index < children.length; index++) {
      const child = children[index]!;
      if (child.type !== "text") continue;
      if (BLOCK_HTML.has(child)) {
        if (BREAKS_ONLY.test(child.value)) {
          const breaks = (child.value.match(BREAK) ?? []).map(() => element("br"));
          children.splice(index, 1, ...breaks);
          index += breaks.length - 1;
        }
      } else if (INLINE_HTML.has(child)) found = true;
    }
    if (found)
      parent.children = paired(children, open?.has(parent) ?? false, MAX_DEPTH - depth) as never;
    for (const child of parent.children)
      if (child.type === "element") stack.push([child, depth + 1]);
  }
}
