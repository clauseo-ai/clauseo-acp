// One streaming answer as blocks of sanitised hast: the per-update hot path. Each update splits the text
// (split.ts), gathers the document-wide link reference definitions (definitions.ts) and footnotes
// (footnotes.ts), and turns each block's parse input into sanitised hast, cached by that input and its
// footnote numbers, so a finished block costs a map lookup and keeps the same view object (the React layer
// memoises on it). Only the streaming last block gets tail repair (repair.ts). The notes, if any, are one
// more view after the last block.
import type { Root as Hast, Text } from "hast";
import type { Root } from "mdast";
import { sanitize } from "hast-util-sanitize";
import { toHast } from "mdast-util-to-hast";
import type { Handler, Options } from "mdast-util-to-hast";
import {
  definitionLines,
  definitionsIn,
  labelsIn,
  mayDefine,
  mayReference,
  prefixFor,
} from "./definitions.ts";
import type { DefinitionLine, DefinitionLines } from "./definitions.ts";
import {
  footnoteLabelsIn,
  footnotesIn,
  identifierOf,
  NO_FOOTNOTES,
  notesHast,
  numberFootnotes,
  placeholdersFor,
  referenceHandler,
  withoutPlaceholders,
} from "./footnotes.ts";
import type { Footnotes, Numbering } from "./footnotes.ts";
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
import { repairTail } from "./repair.ts";
import { SCHEMA } from "./schema.ts";
import { createSplitter } from "./split.ts";
import type { BlockEnd } from "./split.ts";

/**
 * One block, ready to render: `source` is the block's text, `input` what was parsed for it (with its
 * footnote numbers, for the notes view a key), `hast` the sanitised result. `streaming` marks the last
 * block of a streaming answer, whose links stay inert until it is finished (a URL may still be arriving,
 * and tail repair may have rewritten it). `notes` marks the answer's notes, after its last block.
 */
export type BlockView = {
  readonly source: string;
  readonly input: string;
  readonly hast: Hast;
  readonly streaming: boolean;
  readonly notes?: true;
};

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
 * What one block needs for its view: its parse input with every footnote label it may call (`input`, from
 * offset `from`), its footnotes, its labels, and the view's input and cache key of the last update.
 */
type Entry = {
  readonly source: string;
  readonly input: string;
  readonly from: number;
  readonly labels: readonly string[];
  readonly footnotes: Footnotes;
  key: string;
  viewInput: string;
};

/** A cache key for an object, by identity. */
function createIds() {
  const ids = new WeakMap<object, number>();
  let next = 0;
  return (value: object) => {
    let id = ids.get(value);
    if (id === undefined) ids.set(value, (id = next++));
    return id;
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
  const noteLabels = createCache<readonly string[]>();
  const inputs = createCache<{ prefix: string; input: string }>();
  const facts = createCache<Footnotes>();
  const repairs = createCache<{ text: string | undefined }>();
  const views = createCache<BlockView>();
  const tails = createCache<BlockView>();
  const caches = [trees, ends, owned, labels, noteLabels, inputs, facts, repairs, views, tails];
  const idOf = createIds();
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
  const options: Options = { clobberPrefix: `${scope}-`, handlers: { html: htmlAsText } };

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
  const toView = (source: string, input: string, footnote: Handler): BlockView => {
    const shown = restore(source);
    const fallback = { source: shown, input, hast: plainText(shown), streaming: false };
    try {
      const root = withoutPlaceholders(tree(input), input);
      if (deeperThan(root, MAX_TREE_DEPTH)) return fallback;
      const handlers = { html: htmlAsText, footnoteReference: footnote };
      const hast = toHast(root, { ...options, handlers }) as Hast;
      if (hasStandIns(input)) restoreText(hast);
      return { source: shown, input, hast: sanitize(hast, SCHEMA) as Hast, streaming: false };
    } catch {
      return fallback;
    }
  };

  /**
   * The document's link definitions, from each block's own; while streaming, the last block's do not count
   * yet. Built again only when some block's definitions changed.
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
   * A block's parse input: placeholders for the footnote labels `called`, the definitions of the link
   * labels it uses, then the block. The same prefix gives the same input string, so a finished block's
   * lookups stay cheap.
   */
  const inputOf = (source: string, lines: DefinitionLines, called: readonly string[]): string => {
    const links =
      lines.size === 0 || !mayReference(source)
        ? ""
        : prefixFor(
            labels.get(source, () => labelsIn(source)),
            lines,
          );
    const prefix = placeholdersFor(called) + links;
    if (prefix === "") return source;
    const known = inputs.get(source, () => ({ prefix, input: prefix + source }));
    if (known.prefix !== prefix) {
      known.prefix = prefix;
      known.input = prefix + source;
    }
    return known.input;
  };

  /** A block's entry: its input with every label it may call, and the footnotes its parse shows. */
  const entryOf = (source: string, lines: DefinitionLines): Entry => {
    const called = noteLabels.get(source, () => footnoteLabelsIn(source));
    const input = inputOf(source, lines, called);
    const from = input.length - source.length;
    const footnotes =
      called.length === 0
        ? NO_FOOTNOTES
        : facts.get(input, () => footnotesIn(tree(input), input, from));
    return { source, input, from, labels: called, footnotes, key: input, viewInput: input };
  };

  /** The streaming last block's entry: its text after tail repair. */
  const tailEntryOf = (source: string, lines: DefinitionLines): Entry => {
    const raw = entryOf(source, lines);
    const repaired = repairs.get(raw.input, () => ({
      text: repairTail(raw.input, raw.from, tree),
    })).text;
    return repaired === undefined ? raw : entryOf(repaired, lines);
  };

  /**
   * The input a block's view is parsed from: while streaming, the one with every label it may call (an
   * undefined one shows as a pending number); at the end, only the defined ones, so the rest are text.
   */
  const viewInputOf = (
    entry: Entry,
    numbering: Numbering,
    lines: DefinitionLines,
    streaming: boolean,
  ) => {
    if (streaming || entry.labels.every((label) => numbering.notes.has(identifierOf(label))))
      return entry.input;
    const defined = entry.labels.filter((label) => numbering.notes.has(identifierOf(label)));
    return inputOf(entry.source, lines, defined);
  };

  /** The notes view, rebuilt only when a note, its number or its references change. */
  let lastNotes: BlockView | undefined;
  const notesOf = (numbering: Numbering, streamingNotes: Footnotes): BlockView | undefined => {
    let key = "";
    let source = "";
    let streaming = false;
    for (const id of numbering.order) {
      const note = numbering.notes.get(id);
      if (!note) continue;
      key += `${idOf(note.node)}:${numbering.number.get(id)},${numbering.total.get(id)}`;
      for (const [inner, seen] of numbering.inNotes.get(id) ?? [])
        key += `;${numbering.number.get(inner) ?? ""},${seen}`;
      key += "\u0001";
      source += note.source;
      streaming ||= streamingNotes.notes.includes(note);
    }
    if (key === "") return (lastNotes = undefined);
    if (lastNotes?.input === key && lastNotes.streaming === streaming) return lastNotes;
    let hast: Hast;
    try {
      const notes = notesHast(scope, numbering, options);
      hast = notes ? (sanitize(notes, SCHEMA) as Hast) : plainText(source);
    } catch {
      hast = plainText(source);
    }
    return (lastNotes = { source, input: key, hast, streaming, notes: true });
  };

  let lastText: string | undefined;
  let lastStreaming = true;
  let lastViews: readonly BlockView[] = [];
  let lastBlocks: readonly string[] = [];
  let lastEntries: readonly Entry[] = [];
  let lastSignatures: readonly string[] = [];
  let lastLinesUsed: DefinitionLines | undefined;
  return {
    update(raw, streaming) {
      if (!entities && characterEntitiesLoaded()) {
        // The full table arrived: blocks holding `&` are parsed again, the rest keep their views.
        entities = true;
        if (missed) {
          for (const cache of [trees, owned, facts, repairs, views, tails]) cache.forget("&");
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
      // A block finished last time, unchanged, under the same definitions: its entry as it was.
      const reusable = lines === lastLinesUsed ? lastBlocks.length - (lastStreaming ? 1 : 0) : 0;
      const entries = blocks.map((block, index): Entry => {
        const before = lastEntries[index];
        if (streaming && index === last) return tailEntryOf(block, lines);
        if (before && index < reusable && block === lastBlocks[index]) {
          for (const cache of [noteLabels, labels, inputs]) cache.keep(block);
          facts.keep(before.input);
          // Kept parsed: a note arriving re-renders the blocks that call it without parsing them again.
          if (before.footnotes.refs.length > 0) trees.keep(before.input);
          return before;
        }
        return entryOf(block, lines);
      });
      const numbering = numberFootnotes(
        entries.map((entry) => entry.footnotes),
        streaming,
      );
      const previous = lastViews;
      const blockViews = entries.map((entry, index) => {
        const { signature, before } = numbering.blocks[index]!;
        const tail = streaming && index === last;
        const old = previous[index];
        if (
          old &&
          !tail &&
          entry === lastEntries[index] &&
          signature === lastSignatures[index] &&
          streaming === lastStreaming
        ) {
          views.keep(entry.key);
          return old;
        }
        const viewInput = viewInputOf(entry, numbering, lines, streaming);
        entry.viewInput = viewInput;
        const key = keyOf(entry.source, viewInput);
        entry.key = signature === "" ? key : `${key}\u0002${signature}`;
        const make = () =>
          toView(entry.source, viewInput, referenceHandler(scope, numbering, before));
        if (!tail) return views.get(entry.key, make);
        return tails.get(entry.key, () => ({ ...make(), streaming: true }));
      });
      const notes = notesOf(numbering, streaming ? entries[last]!.footnotes : NO_FOOTNOTES);
      lastViews = notes ? [...blockViews, notes] : blockViews;
      lastBlocks = blocks;
      lastEntries = entries;
      lastSignatures = numbering.blocks.map((block) => block.signature);
      lastLinesUsed = lines;
      lastText = raw;
      lastStreaming = streaming;
      for (const cache of caches) cache.sweep();
      return lastViews;
    },
  };
}
