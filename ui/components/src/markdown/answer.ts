// One streaming answer as blocks of sanitised hast: the per-update hot path. Each update splits the text
// (split.ts), gathers the document-wide link reference definitions (definitions.ts), and turns each block's
// parse input into sanitised hast, cached by that input, so a finished block costs a map lookup and keeps
// the same view object (the React layer memoises on it). Only the streaming last block gets tail repair.
//
// Footnotes (later): the place to plug them in is `documentOf` (gather footnote definitions and the order
// of first reference across blocks, like the link definitions here), `inputOf` (placeholder definitions for
// the labels a block references, found like its link labels, so its view changes only when they do) and
// the mdast-to-hast handlers in `toView`.
import type { ElementContent, Root as Hast, RootContent, Text } from "hast";
import type { Root } from "mdast";
import { sanitize } from "hast-util-sanitize";
import { toHast } from "mdast-util-to-hast";
import type { Handler } from "mdast-util-to-hast";
import remend from "remend";
import {
  definitionLines,
  definitionsIn,
  labelsIn,
  mayDefine,
  mayReference,
  prefixFor,
} from "./definitions.ts";
import type { DefinitionLine, DefinitionLines } from "./definitions.ts";
import { characterEntitiesLoaded, characterReferenceMisses } from "./entities.ts";
import {
  capNesting,
  deeperThan,
  hasStandIns,
  MAX_TREE_DEPTH,
  restore,
  restoreText,
} from "./nesting.ts";
import { endOf, parse } from "./parse.ts";
import { SCHEMA } from "./schema.ts";
import { createSplitter } from "./split.ts";
import type { BlockEnd } from "./split.ts";

/**
 * One block, ready to render: `source` is the block's text, `input` what was parsed for it, `hast` the
 * sanitised result. `streaming` marks the last block of a streaming answer, whose links stay inert until it
 * is finished (a URL may still be arriving, and tail repair may have rewritten it).
 */
export type BlockView = {
  readonly source: string;
  readonly input: string;
  readonly hast: Hast;
  readonly streaming: boolean;
};

/**
 * Tail repair for the streaming last block: remend's defaults, except that a half-arrived HTML tag is not
 * stripped (`if a<b then…` showed `if a`; raw HTML is text here, so it cannot restyle what follows) and no
 * `$$` is closed (we render no maths; `https://a.com/$$x` gained `$$`).
 */
const TAIL_REPAIR = { htmlTags: false, katex: false } as const;

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
  return {
    type: "root",
    children: [{ type: "element", tagName: "p", properties: {}, children: [text] }],
  };
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
    /** Mark `key` as used by this update without looking it up. */
    keep(key: string) {
      used.add(key);
    },
    /** Forget every entry whose key holds `text`. */
    forget(text: string) {
      for (const key of map.keys()) if (key.includes(text)) map.delete(key);
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
  const owned = createCache<readonly DefinitionLine[]>();
  const labels = createCache<readonly string[]>();
  const inputs = createCache<{ prefix: string; input: string }>();
  const views = createCache<BlockView>();
  const tails = createCache<BlockView>();
  // Whether a parse met a named character reference before the full table had loaded (entities.ts).
  let missed = false;
  let entities = characterEntitiesLoaded();
  const tree = (source: string) =>
    trees.get(source, () => {
      const misses = characterReferenceMisses();
      const root = parse(source);
      if (!entities && characterReferenceMisses() !== misses) missed = true;
      return root;
    });
  const split = createSplitter((block) => ends.get(block, () => endOf(block, tree(block))));
  const options = { clobberPrefix: `${scope}-`, handlers: { html: htmlAsText } };

  /**
   * A view's cache key: its parse input, and its source when that differs, so a finished block never
   * keeps the view (and the fallback text) of the streaming text that parsed the same.
   */
  const keyOf = (source: string, input: string) =>
    source === input ? input : `${input}\u0000${source}`;

  /**
   * A block's view. A tree too deep to walk safely, or any error on the way, shows the block's source as
   * plain text instead: one bad block never takes the answer down (markdown.tsx adds an error boundary).
   * `source` and `input` are as capped (nesting.ts); the view's `source` and text are as written.
   */
  const toView = (source: string, input: string): BlockView =>
    views.get(keyOf(source, input), () => {
      const shown = restore(source);
      const fallback = { source: shown, input, hast: plainText(shown), streaming: false };
      try {
        const root = tree(input);
        if (deeperThan(root, MAX_TREE_DEPTH)) return fallback;
        const hast = toHast(root, options) as Hast;
        if (input.includes("[^")) scopeFootnoteLabel(hast.children, scope);
        if (hasStandIns(input)) restoreText(hast);
        return { source: shown, input, hast: sanitize(hast, SCHEMA) as Hast, streaming: false };
      } catch {
        return fallback;
      }
    });

  /** The streaming last block's view: the same hast, its links inert. */
  const toTailView = (source: string, input: string): BlockView =>
    tails.get(keyOf(source, input), () => ({ ...toView(source, input), streaming: true }));

  /**
   * The document's definitions, from each block's own; while streaming, the last block's do not count yet.
   * Built again only when some block's definitions changed.
   */
  let lastPerBlock: (readonly DefinitionLine[])[] = [];
  let lastLines: DefinitionLines = new Map();
  const documentOf = (blocks: readonly string[], counted: number): DefinitionLines => {
    const perBlock: (readonly DefinitionLine[])[] = [];
    for (let index = 0; index < counted; index++) {
      const block = blocks[index]!;
      if (mayDefine(block))
        perBlock.push(owned.get(block, () => definitionsIn(tree(block), block)));
    }
    const same =
      perBlock.length === lastPerBlock.length &&
      perBlock.every((definitions, index) => definitions === lastPerBlock[index]);
    if (!same) lastLines = definitionLines(perBlock);
    lastPerBlock = perBlock;
    return lastLines;
  };

  /**
   * A block's parse input: the block, after the definitions of the labels it uses. The same prefix gives
   * the same input string, so a finished block's lookups stay cheap.
   */
  const inputOf = (source: string, lines: DefinitionLines): string => {
    if (lines.size === 0 || !mayReference(source)) return source;
    const prefix = prefixFor(
      labels.get(source, () => labelsIn(source)),
      lines,
    );
    if (prefix === "") return source;
    const known = inputs.get(source, () => ({ prefix, input: prefix + source }));
    if (known.prefix !== prefix) {
      known.prefix = prefix;
      known.input = prefix + source;
    }
    return known.input;
  };

  let lastText: string | undefined;
  let lastStreaming = true;
  let lastViews: readonly BlockView[] = [];
  let lastBlocks: readonly string[] = [];
  let lastLinesUsed: DefinitionLines | undefined;
  return {
    update(raw, streaming) {
      if (!entities && characterEntitiesLoaded()) {
        // The full table arrived: blocks holding `&` are parsed again, the rest keep their views.
        entities = true;
        if (missed) {
          for (const cache of [trees, owned, views, tails]) cache.forget("&");
          lastLinesUsed = undefined;
          lastText = undefined;
        }
      }
      if (raw === lastText && streaming === lastStreaming) return lastViews;
      const text = capNesting(raw.includes("\r") ? raw.replace(/\r\n?/g, "\n") : raw);
      let blocks: readonly string[];
      try {
        blocks = split(text);
      } catch {
        blocks = [text];
      }
      const last = blocks.length - 1;
      const lines = documentOf(blocks, streaming ? last : blocks.length);
      // A block finished last time, unchanged, under the same definitions: its view as it was.
      const reusable = lines === lastLinesUsed ? lastBlocks.length - (lastStreaming ? 1 : 0) : 0;
      const previous = lastViews;
      lastViews = blocks.map((block, index) => {
        const tail = streaming && index === last;
        const before = previous[index];
        if (before && index < reusable && !tail && block === lastBlocks[index]) {
          views.keep(keyOf(block, before.input));
          return before;
        }
        if (!tail) return toView(block, inputOf(block, lines));
        return toTailView(block, inputOf(remend(block, TAIL_REPAIR), lines));
      });
      lastBlocks = blocks;
      lastLinesUsed = lines;
      lastText = raw;
      lastStreaming = streaming;
      for (const cache of [trees, ends, owned, labels, inputs, views, tails]) cache.sweep();
      return lastViews;
    },
  };
}
