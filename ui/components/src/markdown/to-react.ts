// Sanitised hast to React elements: our own mapper over exactly the tags and attributes the sanitise schema
// (schema.ts) allows, in place of hast-util-to-jsx-runtime (about 9 KB brotli with its style parser).
// Links, images and code blocks map to our components; every other allowed tag to its own element. What it
// does not know it drops, whatever the sanitiser let through: an unknown tag with everything inside it, an
// unknown property, a property value that is not a string, number or boolean (or a list of them), and every
// node that is not an element or text (comments, doctypes, raw HTML). Text goes to React as a string child,
// which React escapes; nothing here sets inner HTML. Keys are the tag and its count among its siblings, as
// hast-util-to-jsx-runtime made them, so a re-rendered block keeps its elements.
import type { Element, ElementContent, Properties, Root } from "hast";
import { Fragment, jsx, jsxs } from "react/jsx-runtime";
import type { ReactNode } from "react";
import { CodeBlock } from "./code.tsx";
import { Image } from "./image.tsx";
import { LinkOrReference } from "./link.tsx";

/** hast property name to React prop name, per tag: the schema's attributes, and only those. */
const ATTRIBUTES: Readonly<Record<string, Readonly<Record<string, string>>>> = {
  a: {
    href: "href",
    id: "id",
    ariaDescribedBy: "aria-describedby",
    ariaLabel: "aria-label",
    dataFootnoteRef: "data-footnote-ref",
    dataFootnoteBackref: "data-footnote-backref",
    className: "className",
  },
  code: { className: "className" },
  img: { src: "src", alt: "alt" },
  input: { type: "type", disabled: "disabled", checked: "checked" },
  li: { id: "id", className: "className" },
  ol: { start: "start", className: "className" },
  ul: { className: "className" },
  section: { dataFootnotes: "data-footnotes", className: "className" },
  h2: { id: "id", className: "className" },
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
    "table",
    "thead",
    "tbody",
    "tr",
    "th",
    "td",
    "sup",
    "section",
  ].map((tag): [string, unknown] => [tag, tag]),
  ["a", LinkOrReference],
  ["img", Image],
  ["pre", CodeBlock],
]);

/** Tags React renders without children (it throws on a void element given any). */
const VOID = new Set(["br", "hr", "img", "input"]);
/** Tags whose whitespace-only text children React would warn about (no text node may sit there in HTML). */
const TABLE_PARTS = new Set(["table", "thead", "tbody", "tfoot", "tr"]);
/** Our components that read their hast element (passNode). */
const NEEDS_NODE = new Set(["a", "pre"]);
/** The cell alignments mdast-util-to-hast produces. */
const ALIGNMENTS = new Set(["left", "right", "center"]);
const WHITESPACE = /^[ \t\n\f\r]*$/;

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

/** Text and allowed elements of `nodes`; inside table parts, text that is only whitespace goes. */
function childrenOf(nodes: readonly ElementContent[], table: boolean): ReactNode[] {
  const children: ReactNode[] = [];
  const counts = new Map<string, number>();
  for (const child of nodes) {
    if (child.type === "text") {
      if (!table || !WHITESPACE.test(child.value)) children.push(child.value);
    } else if (child.type === "element") {
      const count = counts.get(child.tagName) ?? 0;
      counts.set(child.tagName, count + 1);
      const element = elementOf(child, `${child.tagName}-${count}`);
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

function elementOf(node: Element, key: string): ReactNode {
  const type = TAGS.get(node.tagName);
  if (type === undefined) return null;
  const props = propsOf(node);
  if (NEEDS_NODE.has(node.tagName)) props["node"] = node;
  // A code block reads its text from the node; its children would be built only to be thrown away.
  const children =
    VOID.has(node.tagName) || node.tagName === "pre"
      ? []
      : childrenOf(node.children, TABLE_PARTS.has(node.tagName));
  return create(type, props, children, key);
}

/** One block's sanitised hast as React elements. */
export function toReact(root: Root): ReactNode {
  return create(Fragment, {}, childrenOf(root.children as ElementContent[], false));
}
