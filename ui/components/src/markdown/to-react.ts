// Sanitised hast to React elements: our own mapper over exactly the tags and attributes the sanitise schema
// (schema.ts) allows, in place of hast-util-to-jsx-runtime (about 9 KB brotli with its style parser).
// Links, images, code blocks, superscripts (footnote numbers), tables (in a box of their own that scrolls)
// and key caps map to our components; every other allowed tag to its own element. What it does not know it drops, whatever the sanitiser let through: an unknown
// tag with everything inside it, an unknown property, a property value that is not a string, number or
// boolean (or a list of them), and every node that is not an element or text (comments, doctypes, raw
// HTML). Text goes to React as a string child,
// which React escapes; nothing here sets inner HTML. Keys are the tag and its count among its siblings, as
// hast-util-to-jsx-runtime made them, so a re-rendered block keeps its elements.
import type { Element, ElementContent, Properties, Root } from "hast";
import { Fragment, jsx, jsxs } from "react/jsx-runtime";
import type { ReactNode } from "react";
import { CodeBlock } from "./code.tsx";
import { Image } from "./image.tsx";
import { LinkOrReference } from "./link.tsx";
import { Key, Table } from "./blocks.tsx";
import { Superscript } from "./notes.tsx";
import { langOfText } from "../lib/script.ts";

/** hast property name to React prop name, per tag: the schema's attributes, and only those. */
const ATTRIBUTES: Readonly<Record<string, Readonly<Record<string, string>>>> = {
  a: {
    href: "href",
    id: "id",
    ariaDescribedBy: "aria-describedby",
    dataFootnoteRef: "data-footnote-ref",
    dataFootnoteBackref: "data-footnote-backref",
    className: "className",
  },
  code: { className: "className" },
  h1: { id: "id" },
  h2: { id: "id" },
  h3: { id: "id" },
  h4: { id: "id" },
  h5: { id: "id" },
  h6: { id: "id" },
  img: { src: "src", alt: "alt" },
  input: { type: "type", disabled: "disabled", checked: "checked" },
  li: { id: "id", className: "className" },
  ol: { start: "start", className: "className" },
  ul: { className: "className" },
  sup: { dataFootnotePending: "data-footnote-pending" },
};

/** Every tag the schema allows, and what renders it: our component, or the tag itself. */
const TAGS: ReadonlyMap<string, unknown> = new Map<string, unknown>([
  ...[
    "p",
    "blockquote",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "code",
    "br",
    "em",
    "strong",
    "del",
    "ul",
    "ol",
    "li",
    "input",
    "thead",
    "tbody",
    "tr",
    "th",
    "td",
    "sub",
    "u",
    "ins",
    "s",
    "mark",
    "small",
    "dl",
    "dt",
    "dd",
  ].map((tag): [string, unknown] => [tag, tag]),
  ["a", LinkOrReference],
  ["table", Table],
  ["kbd", Key],
  ["img", Image],
  ["pre", CodeBlock],
  ["sup", Superscript],
]);

/** Tags React renders without children (it throws on a void element given any). */
const VOID = new Set(["br", "hr", "img", "input"]);
/** Tags whose whitespace-only text children React would warn about (no text node may sit there in HTML). */
const TABLE_PARTS = new Set(["table", "thead", "tbody", "tfoot", "tr"]);
/** The cell alignments mdast-util-to-hast produces. */
const ALIGNMENTS = new Set(["left", "right", "center"]);
const WHITESPACE = /^[ \t\n\f\r]*$/;
/**
 * The blocks that take lang="hi" when their letters are mostly Devanagari (lib/script.ts), in either interface
 * language, so they set in the Hindi type (tokens-type.css). A list item reads its own words, not its sublists'.
 */
const TEXT_BLOCKS = new Set([
  "p",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "li",
  "th",
  "td",
  "dt",
  "dd",
]);
const SUBLISTS = new Set(["ul", "ol"]);

/** The tags and hast properties this mapper renders, for the schema test (every allowed one must be here). */
export const MAPPED = {
  tags: [...TAGS.keys()],
  attributes: (tag: string) => [
    ...Object.keys(ATTRIBUTES[tag] ?? {}),
    ...(tag === "th" || tag === "td" ? ["align"] : []),
  ],
};

/** A property's value as React gets it, or undefined to drop it. */
function valueOf(value: Properties[string]): string | number | boolean | undefined {
  if (typeof value === "string" || typeof value === "boolean") return value;
  if (typeof value === "number") return Number.isNaN(value) ? undefined : value;
  if (Array.isArray(value)) return value.join(" ");
  return undefined;
}

function propsOf(node: Element): Record<string, unknown> {
  const props: Record<string, unknown> = {};
  const { tagName, properties } = node;
  const names = ATTRIBUTES[tagName];
  if (names) {
    for (const name in properties) {
      const prop = Object.hasOwn(names, name) ? names[name] : undefined;
      if (prop === undefined) continue;
      const value = valueOf(properties[name]);
      if (value !== undefined) props[prop] = value;
    }
  } else if (tagName === "th" || tagName === "td") {
    const align = properties["align"];
    if (typeof align === "string" && ALIGNMENTS.has(align)) props["style"] = { textAlign: align };
  }
  return props;
}

/**
 * Text and allowed elements of `nodes`; inside table parts, text that is only whitespace goes. `marked`,
 * one of them, gets CONTINUES.
 */
function childrenOf(
  nodes: readonly ElementContent[],
  table: boolean,
  marked?: ElementContent,
): ReactNode[] {
  const children: ReactNode[] = [];
  const counts = new Map<string, number>();
  for (const child of nodes) {
    if (child.type === "text") {
      if (!table || !WHITESPACE.test(child.value)) children.push(child.value);
    } else if (child.type === "element") {
      const count = counts.get(child.tagName) ?? 0;
      counts.set(child.tagName, count + 1);
      const element = elementOf(child, `${child.tagName}-${count}`, child === marked);
      if (element !== null) children.push(element);
    }
  }
  return children;
}

function create(
  type: unknown,
  props: Record<string, unknown>,
  children: ReactNode[],
  key?: string,
) {
  if (children.length === 1) props["children"] = children[0];
  else if (children.length > 1) props["children"] = children;
  const make = children.length > 1 ? jsxs : jsx;
  return make(type as never, props, key);
}

/**
 * The text of hast nodes, through any markup (`**good.com**`, `` `good.com` ``); with `skip`, not inside the
 * elements it names.
 */
function textOf(nodes: readonly ElementContent[], skip?: ReadonlySet<string>): string {
  let text = "";
  for (const node of nodes) {
    if (node.type === "text") text += node.value;
    else if (node.type === "element" && !skip?.has(node.tagName))
      text += textOf(node.children, skip);
  }
  return text;
}

/**
 * What a code block (`pre`) shows: its `code` child's text and language, as CodeBlock takes them. The
 * elements keep these strings, not the hast: a finished block's view lets its hast go (answer.ts).
 */
function codeOf(node: Element, props: Record<string, unknown>): void {
  const code = node.children.find(
    (child): child is Element => child.type === "element" && child.tagName === "code",
  );
  const classes = code?.properties["className"];
  const language = Array.isArray(classes)
    ? classes.map(String).find((name) => name.startsWith("language-"))
    : undefined;
  props["text"] = textOf(code ? code.children : []);
  if (language) props["lang"] = language.slice("language-".length);
}

function elementOf(node: Element, key: string, continues = false): ReactNode {
  const type = TAGS.get(node.tagName);
  if (type === undefined) return null;
  const props = propsOf(node);
  if (continues) props[CONTINUES.name] = CONTINUES.value;
  // A link's whole text, for its spoofed-host check (link.tsx); a code block's text and language.
  if (node.tagName === "a") props["text"] = textOf(node.children);
  else if (node.tagName === "pre") codeOf(node, props);
  else if (TEXT_BLOCKS.has(node.tagName)) {
    // Once per block as its elements are made: a finished block's are made once and kept (view.ts)
    const lang = langOfText(textOf(node.children, node.tagName === "li" ? SUBLISTS : undefined));
    if (lang !== undefined) props["lang"] = lang;
  }
  // A code block's children would be built only to be thrown away.
  const children =
    VOID.has(node.tagName) || node.tagName === "pre"
      ? []
      : childrenOf(node.children, TABLE_PARTS.has(node.tagName));
  return create(type, props, children, key);
}

/**
 * The attribute and value a paragraph gets when it continues the paragraph of the block before (a cut,
 * BlockView's `continues`): a token the answer's styles take away the gap between paragraphs for
 * (thread.css), so the parts read as one paragraph.
 */
export const CONTINUES = { name: "data-clauseo-paragraph", value: "continues" } as const;

/**
 * One block's sanitised hast as React elements. With `continues`, its first element, when a paragraph,
 * gets CONTINUES (the attribute is ours: the sanitiser lets no `data-` attribute onto a `p`).
 */
export function toReact(root: Root, continues = false): ReactNode {
  const nodes = root.children as ElementContent[];
  const first = nodes.find((child) => child.type === "element");
  const marked = continues && first?.tagName === "p" ? first : undefined;
  return create(Fragment, {}, childrenOf(nodes, false, marked));
}
