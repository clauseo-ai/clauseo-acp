// One streaming answer as blocks of sanitised hast: the per-update hot path. Each update splits the text
// (split.ts), gathers the document-wide link reference definitions (definitions.ts), and turns each block's
// parse input into sanitised hast, cached by that input, so a finished block costs a map lookup and keeps
// the same view object (the React layer memoises on it). Only the streaming last block gets tail repair.
//
// Footnotes (later): the place to plug them in is `documentOf` (gather footnote definitions and the order
// of first reference across blocks, like the link definitions here), `inputOf` (placeholder definitions for
// the labels a block references) and the mdast-to-hast handlers in `toView`.
import type { ElementContent, Root as Hast, RootContent, Text } from "hast";
import type { Definition, Root } from "mdast";
import { sanitize } from "hast-util-sanitize";
import { toHast } from "mdast-util-to-hast";
import type { Handler } from "mdast-util-to-hast";
import remend from "remend";
import { definitionPrefix, definitionsIn, mayDefine, mayReference } from "./definitions.ts";
import { capNesting, deeperThan, MAX_TREE_DEPTH } from "./nesting.ts";
import { endOf, parse } from "./parse.ts";
import { SCHEMA } from "./schema.ts";
import { createSplitter } from "./split.ts";
import type { BlockEnd } from "./split.ts";

/**
 * One block, ready to render: `source` is the block's text, `input` what was parsed for it, `hast` the
 * sanitised result.
 */
export type BlockView = { readonly source: string; readonly input: string; readonly hast: Hast };

export type Answer = {
  /** The blocks for the whole answer so far; `streaming` is false once the turn has ended. */
  update(text: string, streaming: boolean): readonly BlockView[];
};

/** Raw HTML stays text: an HTML node becomes a text node, which React escapes (no rehype-raw anywhere). */
const htmlAsText: Handler = (state, node: { value: string }) => {
  const text: Text = { type: "text", value: node.value };
  state.patch(node as never, text);
  return text;
};

/**
 * remark-rehype gives the footnote section's heading the fixed id `footnote-label` (every reference points
 * at it with aria-describedby); scoped here like every other generated id, so two answers never share it.
 */
function scopeFootnoteLabel(nodes: readonly (RootContent | ElementContent)[], scope: string) {
  for (const node of nodes) {
    if (node.type !== "element") continue;
    const { properties } = node;
    if (properties["id"] === "footnote-label") properties["id"] = `${scope}-footnote-label`;
    const described = properties["ariaDescribedBy"];
    if (Array.isArray(described)) {
      properties["ariaDescribedBy"] = described.map((id) =>
        id === "footnote-label" ? `${scope}-footnote-label` : id,
      );
    }
    scopeFootnoteLabel(node.children, scope);
  }
}

/** A block shown as its source, in plain text: what a block that cannot render safely falls back to. */
export function plainText(source: string): Hast {
  const text: Text = { type: "text", value: source.trimEnd() };
  return { type: "root", children: [{ type: "element", tagName: "p", properties: {}, children: [text] }] };
}

/** A cache that keeps only what the last update used, once it holds more than twice that. */
function createCache<V>() {
  const map = new Map<string, V>();
  let used = new Set<string>();
  return {
    get(key: string, make: () => V): V {
      used.add(key);
      let value = map.get(key);
      if (value === undefined) {
        value = make();
        map.set(key, value);
      }
      return value;
    },
    sweep() {
      if (map.size > 2 * used.size + 16)
        for (const key of map.keys()) if (!used.has(key)) map.delete(key);
      used = new Set();
    },
  };
}

/**
 * The model of one answer. `scope` is the answer's own part of every id it generates (a transcript shows
 * many answers): remark-rehype prefixes generated ids with it, and the sanitiser adds `user-content-`.
 */
export function createAnswer(scope: string): Answer {
  const trees = createCache<Root>();
  const ends = createCache<BlockEnd>();
  const owned = createCache<readonly Definition[]>();
  const views = createCache<BlockView>();
  const tree = (source: string) => trees.get(source, () => parse(source));
  const split = createSplitter((block) => ends.get(block, () => endOf(block, tree(block))));
  const options = { clobberPrefix: `${scope}-`, handlers: { html: htmlAsText } };

  /**
   * A block's view. A tree too deep to walk safely, or any error on the way, shows the block's source as
   * plain text instead: one bad block never takes the answer down (markdown.tsx adds an error boundary).
   */
  const toView = (source: string, input: string): BlockView =>
    views.get(input, () => {
      try {
        const root = tree(input);
        if (deeperThan(root, MAX_TREE_DEPTH)) return { source, input, hast: plainText(source) };
        const hast = toHast(root, options) as Hast;
        if (input.includes("[^")) scopeFootnoteLabel(hast.children, scope);
        return { source, input, hast: sanitize(hast, SCHEMA) as Hast };
      } catch {
        return { source, input, hast: plainText(source) };
      }
    });

  /** Each block's own definitions; while streaming, the last block's do not count yet. */
  const documentOf = (blocks: readonly string[], counted: number) => {
    const perBlock: (readonly Definition[])[] = [];
    for (let index = 0; index < counted; index++) {
      const block = blocks[index]!;
      if (mayDefine(block)) perBlock.push(owned.get(block, () => definitionsIn(tree(block))));
    }
    return { definitions: definitionPrefix(perBlock) };
  };

  const inputOf = (source: string, document: { definitions: string }) =>
    document.definitions !== "" && mayReference(source) ? document.definitions + source : source;

  let lastText: string | undefined;
  let lastStreaming = true;
  let lastViews: readonly BlockView[] = [];
  return {
    update(raw, streaming) {
      if (raw === lastText && streaming === lastStreaming) return lastViews;
      const text = capNesting(raw.includes("\r") ? raw.replace(/\r\n?/g, "\n") : raw);
      let blocks: readonly string[];
      try {
        blocks = split(text);
      } catch {
        blocks = [text];
      }
      const last = blocks.length - 1;
      const document = documentOf(blocks, streaming ? last : blocks.length);
      lastViews = blocks.map((block, index) => {
        const source = streaming && index === last ? remend(block) : block;
        return toView(block, inputOf(source, document));
      });
      lastText = raw;
      lastStreaming = streaming;
      for (const cache of [trees, ends, owned, views]) cache.sweep();
      return lastViews;
    },
  };
}
