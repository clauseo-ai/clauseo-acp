// The approved inline tags (issue #137 §4): sup, sub, u, ins, del, s, mark, br, small and kbd, nothing else.
// Raw HTML is never parsed as HTML. micromark already reads each inline tag as its own html node, which the
// mdast-to-hast handler here turns into a text node (React escapes text). Then, in each element's children,
// an allowlisted opening tag and its closing tag become one of our elements, with no properties: every
// attribute is dropped, whatever it was (`style`, `class`, `on…`). Everything else stays the text it was
// written as, so a lawyer sees every character the agent wrote:
// - Any tag not on the list whose name is not an HTML element's (placeholders like `<Party A>`, `<insert
//   date>`, `<Client>`), an HTML-named tag that is not clearly markup (below), comments, and tags written as
//   entities (`&lt;sup&gt;`, which are text already).
// - An opening tag with no closing tag in the same parent (a tag cannot pair across a paragraph, a list item
//   or an emphasis it is not inside, nor across a block element or a new paragraph among its siblings, as
//   a tight list item or a definition has), a stray closing tag, a self-closed `<sup/>`, `</br>`, and of
//   misnested tags (`<sup><sub>a</sup></sub>`) the inner one.
// - Past MAX_TAG_NESTING allowlisted elements inside each other, or MAX_DEPTH levels of hast, the deeper
//   tags (hostile nesting: the tree is walked recursively later).
// A tag not on the list whose name is an HTML element's (HTML_ELEMENTS: `<div>`, `<span>`, `<center>`) is
// dropped, every attribute with it, and what it held stays where it was, as text and Markdown, but only
// when it is clearly markup: an opening tag that pairs with its closing tag in the same parent (as an
// allowlisted pair would), a self-closed tag, or an opening tag with an attribute that has a value; and the
// closing tag of a dropped pair. Agents use such tags for presentation: in the #137 format battery (48
// answers each from Claude Code and Codex, measurements/137-markdown/format-battery/COUNTS.md in the
// private harness), Codex wrote `<div align="right">A: …<br>B: …</div>` in table cells in 2 answers
// (p01-r3, p15-r1), which as text buries the cell's words in markup. Placeholders must never vanish, and
// some share an HTML element's name: `<Address>`, `<Title>`, `<Section>`, `<Label>`, `<Time>`, and
// `<Address of Tenant>` is a valid tag to micromark (name `address`, two attributes without values). So a
// lone opening tag with no valued attribute, and a closing tag with no dropped opener, stay text.
// The never-markup elements (RAW_CONTENT: script, style, iframe, object, embed, template, noscript, svg,
// math) follow the same rule, and a dropped pair's content is shown as one plain text, never as markup:
// tags in it stay as written, and the Markdown in it as its text. An HTML block (one of kinds 6 and 7; kinds
// 1 to 5 never start, nesting.ts) is text, and the same rule drops its clearly-markup tags; a kind 1 start
// that nesting.ts made text (`<script>` at the start of a line) is a tag again for this rule (markCutTags).
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
import { standIn } from "./nesting.ts";

/** nesting.ts's stand-in for a `<` that would start an HTML block of kinds 1 to 5. */
export const CUT_LESS_THAN = standIn("<");

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

/**
 * HTML element names, fixed here and never inferred: the WHATWG HTML Living Standard (Last Updated
 * 14 October 2026), Index, "Elements" (https://html.spec.whatwg.org/multipage/indices.html#elements-3, 113
 * names, with MathML `math` and SVG `svg`), and the entirely obsolete elements of 16.2 "Non-conforming
 * features" (https://html.spec.whatwg.org/multipage/obsolete.html#non-conforming-features).
 */
export const HTML_ELEMENTS: ReadonlySet<string> = new Set(
  [
    // Index, Elements.
    "a abbr address area article aside audio b base bdi bdo blockquote body br button canvas caption cite",
    "code col colgroup data datalist dd del details dfn dialog div dl dt em embed fieldset figcaption figure",
    "footer form h1 h2 h3 h4 h5 h6 head header hgroup hr html i iframe img input ins kbd label legend li link",
    "main map mark math menu meta meter nav noscript object ol optgroup option output p picture pre progress",
    "q rp rt ruby s samp script search section select selectedcontent slot small source span strong style",
    "sub summary sup svg table tbody td template textarea tfoot th thead time title tr track u ul var video",
    "wbr",
    // 16.2, obsolete elements.
    "applet acronym bgsound dir frame frameset noframes isindex keygen listing menuitem nextid noembed param",
    "plaintext rb rtc strike xmp basefont big blink center font marquee multicol nobr spacer tt",
  ]
    .join(" ")
    .split(" "),
);

/** HTML elements whose content is never markup: a dropped pair's content shows as one plain text. */
export const RAW_CONTENT: ReadonlySet<string> = new Set([
  "script",
  "style",
  "iframe",
  "object",
  "embed",
  "template",
  "noscript",
  "svg",
  "math",
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
  if (PARAGRAPH_START.has(node)) return true;
  if (node.type === "element") return BLOCKS.has(node.tagName);
  return node.type === "text" && BLOCK_HTML.has(node);
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

/** CommonMark's open and closing tags (6.6), with groups: name, attributes, `/`; or a closing tag's name. */
const ATTRIBUTE = String.raw`[ \t\n]+[A-Za-z_:][A-Za-z0-9_.:-]*(?:[ \t\n]*=[ \t\n]*(?:[^"'=<>\`\s]+|'[^']*'|"[^"]*"))?`;
const RAW_TAG = String.raw`<([A-Za-z][A-Za-z0-9-]*)((?:${ATTRIBUTE})*)[ \t\n]*(\/?)>|<\/([A-Za-z][A-Za-z0-9-]*)[ \t\n]*>`;
const WHOLE_TAG = new RegExp(`^(?:${RAW_TAG})$`);
const TAG_AT = new RegExp(RAW_TAG, "y");

/** A tag not on the list whose name is an HTML element's: its name, kind and whether a value is set. */
type HtmlTag = Tag & { valued: boolean };

/** The HtmlTag a match of RAW_TAG is, or undefined. */
function htmlTagFrom(match: RegExpExecArray | null): HtmlTag | undefined {
  if (!match) return undefined;
  const name = (match[1] ?? match[4])!.toLowerCase();
  if (!HTML_ELEMENTS.has(name) || ALLOWED_TAGS.has(name)) return undefined;
  // `=` is in no attribute name nor unquoted value, and a quoted value comes after one.
  const valued = (match[2] ?? "").includes("=");
  return { name, close: match[4] !== undefined, selfClosed: match[3] === "/", valued };
}

/**
 * Of `tags` (one per node or token, undefined for the rest), what HTML_ELEMENTS' rule drops: the
 * never-markup pairs (opening tag index to closing tag index), whose content shows as text, and every
 * other tag that is clearly markup (header). Never-markup pairs are found first; a tag inside one is not
 * looked at.
 */
function markupIn(
  tags: readonly (HtmlTag | undefined)[],
  barriers: readonly boolean[],
): { raw: Map<number, number>; dropped: Set<number> } {
  const raw = pairsIn(
    tags.map((tag) => (tag && RAW_CONTENT.has(tag.name) ? tag : undefined)),
    barriers,
  ).pairs;
  // Pairs nest or are apart (pairsIn); only the outermost count. What they span is no tag any more.
  let outerEnd = -1;
  for (const start of [...raw.keys()].sort((a, b) => a - b)) {
    if (start < outerEnd) raw.delete(start);
    else outerEnd = raw.get(start)!;
  }
  const rest = [...tags];
  for (const [start, end] of raw) rest.fill(undefined, start, end + 1);
  const dropped = new Set<number>();
  for (const [start, end] of pairsIn(rest, barriers).pairs) dropped.add(start).add(end);
  rest.forEach((tag, index) => {
    if (tag && !tag.close && (tag.selfClosed || tag.valued)) dropped.add(index);
  });
  return { raw, dropped };
}

/**
 * A hast node's text: its value, an image's alt text, or its descendants' text. The id of each footnote
 * reference it holds goes to `lost`: shown as its number only, it is no link target any more.
 */
function textOf(node: ElementContent, lost: string[]): string {
  if (node.type === "text") return node.value;
  if (node.type !== "element") return "";
  const { alt, id, dataFootnoteRef } = node.properties;
  if (node.tagName === "img") return typeof alt === "string" ? alt : "";
  if (node.tagName === "a" && dataFootnoteRef && typeof id === "string") lost.push(id);
  return node.children.map((child) => textOf(child, lost)).join("");
}

/**
 * `nodes` with HTML_ELEMENTS' rule applied: a dropped tag's text node is emptied (kept, so a paragraph's
 * first node stays one), and a never-markup pair is one text node of what it held (textOf, the ids of the
 * footnote references in it to `lost`). What becomes text here is no tag any more for the allowlist.
 */
function dropMarkup(nodes: ElementContent[], lost: string[]): ElementContent[] {
  const tags = nodes.map((node) =>
    node.type === "text" && INLINE_HTML.has(node)
      ? htmlTagFrom(WHOLE_TAG.exec(node.value))
      : undefined,
  );
  if (!tags.some(Boolean)) return nodes;
  const barriers = nodes.map((node, index) => index > 0 && isBarrier(node));
  const { raw, dropped } = markupIn(tags, barriers);
  for (const index of dropped) {
    const node = nodes[index] as Text;
    INLINE_HTML.delete(node);
    node.value = "";
  }
  if (raw.size === 0) return nodes;
  const out: ElementContent[] = [];
  for (let index = 0; index < nodes.length; index++) {
    const node = nodes[index]!;
    const end = raw.get(index);
    if (end === undefined) {
      out.push(node);
      continue;
    }
    const inner = nodes.slice(index + 1, end);
    for (const child of [...inner, nodes[end]!])
      if (child.type === "text") INLINE_HTML.delete(child);
    const text = node as Text;
    INLINE_HTML.delete(text);
    text.value = inner.map((child) => textOf(child, lost)).join("");
    out.push(text);
    index = end;
  }
  return out;
}

/** An HTML block's text with HTML_ELEMENTS' rule applied (its tags never pair with the allowlist). */
function dropBlockMarkup(value: string): string {
  if (!value.includes("<")) return value;
  const found: { start: number; end: number; tag: HtmlTag | undefined }[] = [];
  for (let at = value.indexOf("<"); at >= 0; at = value.indexOf("<", at + 1)) {
    TAG_AT.lastIndex = at;
    const match = TAG_AT.exec(value);
    if (!match) continue;
    found.push({ start: at, end: TAG_AT.lastIndex, tag: htmlTagFrom(match) });
    at = TAG_AT.lastIndex - 1;
  }
  const { raw, dropped } = markupIn(
    found.map((token) => token.tag),
    found.map(() => false),
  );
  for (const [start, end] of raw) dropped.add(start).add(end);
  let out = "";
  let copied = 0;
  for (const index of [...dropped].sort((a, b) => a - b)) {
    out += value.slice(copied, found[index]!.start);
    copied = found[index]!.end;
  }
  return out + value.slice(copied);
}

/**
 * The tags nesting.ts made text at the start of a line (its stand-in for `<` before `script`, `pre`,
 * `style` or `textarea`, which would start an HTML block running to its end tag), as inline HTML again,
 * so HTML_ELEMENTS' rule sees them as it sees the same tag anywhere else. Before restoreText, outside code.
 * A tag is what micromark would read as one, so it is matched on the source the text node was parsed from
 * (`source`, at the node's position), and taken only where the text shows that source as written: a text
 * node's value has its escapes and character references decoded, and `para\n<script &#62;x</script>`
 * matched on it made a tag of what micromark reads as text, and dropped it. `offset` is where `source`
 * starts in what was parsed (a note's text, in its block's input).
 */
export function markCutTags(root: Root, source: string, offset = 0): void {
  const stack: (Root | Element)[] = [root];
  for (let parent = stack.pop(); parent; parent = stack.pop()) {
    const children = parent.children as ElementContent[];
    for (let index = 0; index < children.length; index++) {
      const child = children[index]!;
      if (child.type === "element") {
        if (child.tagName !== "code" && child.tagName !== "pre") stack.push(child);
        continue;
      }
      if (child.type !== "text" || INLINE_HTML.has(child) || BLOCK_HTML.has(child)) continue;
      const pieces = cutTagsIn(child, source, offset);
      if (pieces.length === 0) continue;
      children.splice(index, 1, ...pieces);
      index += pieces.length - 1;
    }
  }
}

/**
 * A text node's value split at the tags its stand-ins start, as micromark would read them from the
 * node's source: each stand-in of the value has its own in the source, in the same order (no escape or
 * reference makes or hides one; micromark turns U+FDD0 to U+FDEF written as references into U+FFFD); a
 * tag there is one where the value shows that tag's text exactly. Text, tag (as inline HTML), text, and so
 * on; none when there is no tag.
 */
function cutTagsIn(node: Text, source: string, offset: number): Text[] {
  const { value } = node;
  let at = value.indexOf(CUT_LESS_THAN);
  if (at < 0) return [];
  const start = (node.position?.start.offset ?? -1) - offset;
  const end = (node.position?.end.offset ?? -1) - offset;
  if (start < 0 || end > source.length) return [];
  const written = source.slice(start, end);
  const pieces: Text[] = [];
  let copied = 0;
  for (let from = written.indexOf(CUT_LESS_THAN); at >= 0 && from >= 0;) {
    TAG_AT.lastIndex = 0;
    // One inside the tag before (in an attribute's value) starts none.
    const match = at >= copied ? TAG_AT.exec(`<${written.slice(from + 1)}`) : null;
    if (match && value.startsWith(match[0].slice(1), at + 1)) {
      const tag: Text = { type: "text", value: match[0] };
      INLINE_HTML.add(tag);
      pieces.push({ type: "text", value: value.slice(copied, at) }, tag);
      copied = at + match[0].length;
    }
    at = value.indexOf(CUT_LESS_THAN, at + 1);
    from = written.indexOf(CUT_LESS_THAN, from + 1);
  }
  if (pieces.length === 0) return [];
  // The first piece keeps the node itself (a paragraph's first node stays one, PARAGRAPH_START).
  node.value = pieces[0]!.value;
  pieces[0] = node;
  pieces.push({ type: "text", value: value.slice(copied) });
  return pieces;
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
 * for the streaming last block, whose unpaired opening tags at its end are hidden. Returns the ids of the
 * footnote references a dropped never-markup pair made text (textOf).
 */
export function pairTags(root: Root, streaming: boolean): string[] {
  const lost: string[] = [];
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
        } else child.value = dropBlockMarkup(child.value);
      } else if (INLINE_HTML.has(child)) found = true;
    }
    if (found)
      parent.children = paired(
        dropMarkup(children, lost),
        open?.has(parent) ?? false,
        MAX_DEPTH - depth,
      ) as never;
    for (const child of parent.children)
      if (child.type === "element") stack.push([child, depth + 1]);
  }
  return lost;
}
