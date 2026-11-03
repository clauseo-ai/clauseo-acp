// One streaming answer as blocks of sanitised hast: the per-update hot path. Each update splits the text
// (split.ts), gathers the document-wide link reference definitions (definitions.ts) and footnotes
// (footnotes.ts), and turns each block's parse input into sanitised hast, cached by that input and its
// footnote numbers, so a finished block costs a map lookup and keeps the same view object (the React layer
// memoises on it). Only the streaming last block gets tail repair (repair.ts). The notes, if any, are one
// more view after the last block. Inline HTML is text but for the allowlisted tags (inline-tags.ts), and
// headings get GitHub's slugs as ids, numbered across the whole answer (slugs.ts). What a view keeps once its
// update has ended, and what parses a finished block again, is view.ts's.
import type { Root as Hast } from "hast";
import type { Root } from "mdast";
import { sanitize } from "hast-util-sanitize";
import { defListHastHandlers } from "mdast-util-definition-list";
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
  definedIn,
  footnoteLabelsIn,
  footnotesIn,
  identifierOf,
  NO_FOOTNOTES,
  noteNumbersOf,
  numberFootnotes,
  placeholdersFor,
  referenceHandler,
  referencesFor,
  withoutPlaceholders,
} from "./footnotes.ts";
import type { Footnotes, Nths, Numbering, References } from "./footnotes.ts";
import { createCache, createIds, createJoiner, createVariants } from "./cache.ts";
import { characterEntitiesLoaded, characterReferenceMisses } from "./entities.ts";
import {
  capNesting,
  deeperThan,
  hasStandIns,
  MAX_TREE_DEPTH,
  restore,
  restoreText,
} from "./nesting.ts";
import {
  CUT_LESS_THAN,
  htmlAsText,
  markCutTags,
  pairTags,
  paragraphNotingStart,
} from "./inline-tags.ts";
import { createAppender } from "./append.ts";
import { endOf, parse } from "./parse.ts";
import { repairTail } from "./repair.ts";
import { CLOBBER_PREFIX, SCHEMA } from "./schema.ts";
import { anchorHeadings, createSlugNumbering, headingId, withSlugs } from "./slugs.ts";
import { createSplitter, joinParts, MAX_PARAGRAPH } from "./split.ts";
import type { BlockEnd } from "./split.ts";
import { extrasHandlers } from "./syntax-extras.ts";
import { noteViewRebuild } from "./note-view.ts";
import { EMPTY_HAST, plainText, seal, variantOf, viewOf } from "./view.ts";
import type { BlockView, Fields } from "./view.ts";

export type { BlockView } from "./view.ts";
export { holdsHast, plainText } from "./view.ts";

export type Answer = {
  /** The blocks for the whole answer so far; `streaming` is false once the turn has ended. */
  update(text: string, streaming: boolean): readonly BlockView[];
  /** Whether a parse met a named character reference the common set doesn't know, and the full table hasn't
   * arrived yet: the answer needs one more render once it does (entities.ts). Read when the table arrives, outside
   * render, so the React compiler never caches it (markdown.tsx). */
  waitsForEntities(): boolean;
};

/** mdast to hast: raw HTML as text, the syntax extras, definition lists. */
export const HANDLERS: Record<string, Handler> = {
  html: htmlAsText,
  paragraph: paragraphNotingStart,
  ...extrasHandlers,
  ...(defListHastHandlers as Record<string, Handler>),
};

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
  /** Some labels it calls got no placeholder, past MAX_PENDING: its input depends on the notes that exist. */
  readonly capped: boolean;
  key: string;
  viewInput: string;
};

/**
 * While streaming, the labels of one block that get a placeholder though no block defines them yet (each
 * shows as a pending number). Every placeholder costs micromark a definition (about 0.1 ms in Node), so a
 * block calling 2,000 labels took 250 ms an update; past this many, the rest show as written until a note
 * for them arrives. An answer's block cites a handful.
 */
const MAX_PENDING = 64;

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
  const defines = createCache<ReturnType<typeof definedIn>>();
  const inputs = createCache<{ prefix: string; input: string }>();
  const facts = createCache<Footnotes>();
  const repairs = createCache<{ text: string | undefined }>();
  const views = createCache<BlockView>();
  const tails = createCache<BlockView>();
  const noteViews = createCache<BlockView>();
  // Not trees: each update keeps only the streaming last block's parses (tailKeys).
  const caches = [
    ends,
    owned,
    labels,
    noteLabels,
    defines,
    inputs,
    facts,
    repairs,
    views,
    tails,
    noteViews,
  ];
  const idOf = createIds();
  // Whether a parse met a named character reference before the full table had loaded (entities.ts).
  let missed = false;
  let entities = characterEntitiesLoaded();
  // The inputs parsed for the streaming last block this update: the only mdast the model keeps after it.
  const tailKeys = new Set<string>();
  let inTail = false;
  // The tail's parses, each the last update's extended when only inert text was added (append.ts).
  const appender = createAppender();
  const tree = (source: string) => {
    if (inTail) tailKeys.add(source);
    const root = trees.get(source, () => {
      const misses = characterReferenceMisses();
      const root = inTail ? appender.parse(source) : parse(source);
      if (!entities && characterReferenceMisses() !== misses) missed = true;
      return root;
    });
    if (inTail) appender.add(source, root);
    return root;
  };
  // The views this update made, sealed as it ends.
  const made: BlockView[] = [];
  const view = (fields: Fields, hast: Hast | undefined, rebuild: () => Hast) => {
    const one = viewOf(fields, hast, rebuild);
    made.push(one);
    return one;
  };
  const variant = (base: BlockView, extra: Partial<Fields>, change?: (hast: Hast) => Hast) => {
    const one = variantOf(base, extra, change);
    made.push(one);
    return one;
  };
  const split = createSplitter((block) => ends.get(block, () => endOf(block, tree(block))));
  const options: Options = { clobberPrefix: `${scope}-`, handlers: HANDLERS };

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
   * `references` and `nths` are what its footnote references read (referencesFor), kept to make its hast
   * again. `written` marks the streaming last block, as written before tail repair: its unpaired tags at its
   * end are hidden, and the view's `source` is that text.
   */
  const toView = (
    source: string,
    input: string,
    references: References,
    nths: Nths,
    written?: string,
  ): BlockView => {
    const tail = written !== undefined;
    const shown = restore(source);
    // `cached`: through the tree cache, as an update parses. A rebuild after the update (view.ts) parses
    // without it, so no finished block's mdast is left in the cache.
    const render = (cached: boolean) => {
      try {
        const parsed = cached ? tree(input) : parse(input);
        const root = withoutPlaceholders(parsed, input.length - source.length);
        if (deeperThan(root, MAX_TREE_DEPTH)) return undefined;
        const footnote = referenceHandler(scope, references, nths);
        const handlers = { ...HANDLERS, footnoteReference: footnote };
        const hast = toHast(root, { ...options, handlers }) as Hast;
        // A `<` nesting.ts made text at a line's start may be a tag again (markCutTags).
        const tags = input.includes("<") || input.includes(CUT_LESS_THAN);
        if (tags) markCutTags(hast, input);
        if (hasStandIns(input)) restoreText(hast);
        const lost = tags ? pairTags(hast, tail) : [];
        const slugs = anchorHeadings(hast, scope);
        return { hast: sanitize(hast, SCHEMA) as Hast, slugs, lost };
      } catch {
        return undefined;
      }
    };
    const fields: Fields = {
      source: tail ? restore(written) : shown,
      input,
      streaming: tail,
    };
    const done = render(true);
    if (!done) return view(fields, plainText(shown), () => plainText(shown));
    const rendered: Fields = { ...fields, slugs: done.slugs };
    return view(
      done.lost.length > 0 ? { ...rendered, lostReferences: done.lost } : rendered,
      done.hast,
      () => render(false)?.hast ?? plainText(shown),
    );
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
      // A block whose only `]:` are footnote definitions holds no link definition: not parsed for one.
      if (
        mayDefine(block) &&
        (!block.includes("[^") || defines.get(block, () => definedIn(block)).links)
      )
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

  /**
   * The identifiers some block may define (definedIn), rebuilt only when a block's set changed: what a
   * finished render gives placeholders for, and what streaming gives one past MAX_PENDING.
   */
  let lastDefines: ReadonlySet<string>[] = [];
  let lastDefined: ReadonlySet<string> = new Set();
  const documentNotesOf = (blocks: readonly string[]): ReadonlySet<string> => {
    const perBlock: ReadonlySet<string>[] = [];
    for (const block of blocks)
      if (block.includes("[^")) {
        const found = defines.get(block, () => definedIn(block)).defines;
        if (found.size > 0) perBlock.push(found);
      }
    const same =
      perBlock.length === lastDefines.length &&
      perBlock.every((found, index) => found === lastDefines[index]);
    if (!same) lastDefined = new Set(perBlock.flatMap((found) => [...found]));
    lastDefines = perBlock;
    return lastDefined;
  };

  /**
   * A block's entry: its input with a placeholder for each label it calls but does not define itself, and
   * the footnotes its parse shows. A finished render needs placeholders only for labels some block may
   * define (the rest are text); streaming, for every label, the undefined ones up to MAX_PENDING. A block
   * that defines all it calls (a note) is parsed as it is, with no placeholder.
   */
  const entryOf = (
    source: string,
    lines: DefinitionLines,
    defined: ReadonlySet<string>,
    streaming: boolean,
  ): Entry => {
    const called = noteLabels.get(source, () => footnoteLabelsIn(source));
    if (called.length === 0) {
      const input = inputOf(source, lines, called);
      const from = input.length - source.length;
      const base = { source, input, from, labels: called, footnotes: NO_FOOTNOTES, capped: false };
      return { ...base, key: input, viewInput: input };
    }
    const { own } = defines.get(source, () => definedIn(source));
    let pending = 0;
    let capped = false;
    const needs = (label: string, trusted: ReadonlySet<string>) => {
      const id = identifierOf(label);
      if (trusted.has(id)) return false;
      if (defined.has(id)) return true;
      if (!streaming) return false;
      if (pending >= MAX_PENDING) {
        capped = true;
        return false;
      }
      pending++;
      return true;
    };
    let needed = called.filter((label) => needs(label, own));
    let input = inputOf(source, lines, needed);
    let footnotes = facts.get(input, () =>
      footnotesIn(tree(input), input, input.length - source.length),
    );
    // A label left out for a note of its own that the parse does not show (one in a code block): again,
    // with it.
    if (own.size > 0 && needed.length < called.length) {
      const shown = new Set(footnotes.notes.map((note) => note.id));
      if ([...own].some((id) => !shown.has(id))) {
        pending = 0;
        capped = false;
        needed = called.filter((label) => needs(label, shown));
        input = inputOf(source, lines, needed);
        const from = input.length - source.length;
        footnotes = facts.get(input, () => footnotesIn(tree(input), input, from));
      }
    }
    const from = input.length - source.length;
    return { source, input, from, labels: needed, footnotes, capped, key: input, viewInput: input };
  };

  /** The streaming last block's entry: its text after tail repair. */
  const tailEntryOf = (
    source: string,
    lines: DefinitionLines,
    defined: ReadonlySet<string>,
  ): Entry => {
    const raw = entryOf(source, lines, defined, true);
    const repaired = repairs.get(raw.input, () => ({
      text: repairTail(raw.input, raw.from, tree),
    })).text;
    if (repaired !== undefined) appender.pending();
    return repaired === undefined ? raw : entryOf(repaired, lines, defined, true);
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

  /**
   * The notes view: one view per shown note, each rebuilt only when its note, its number, its references
   * or the numbers of the references inside it change; the notes view itself only when one of them does.
   * A note still streaming in the last block has inert links, as that block does.
   */
  let lastNotes: BlockView | undefined;
  const noteViewOf = (
    id: string,
    numbering: Numbering,
    streaming: boolean,
    lost: ReadonlySet<string>,
  ): BlockView => {
    const note = numbering.notes.get(id)!;
    let key = `${idOf(note.node)}:${numbering.number.get(id)},${numbering.total.get(id)}`;
    for (const [inner, list] of numbering.inNotes.get(id) ?? []) {
      const known = numbering.notes.has(inner) ? 1 : 0;
      key += `;${inner}\u0000${numbering.number.get(inner) ?? ""},${known},${list.join(".")}`;
    }
    // The back-references it leaves out (lostReferences): any of the answer's change every note's key,
    // which only a reference inside a dropped never-markup pair does.
    if (lost.size > 0) key += `\u0004${[...lost].join(" ")}`;
    return noteViews.get(`${key}\u0002${streaming ? 1 : 0}`, () => {
      const numbers = noteNumbersOf(numbering, id);
      const shown = restore(note.source);
      // Made apart from this scope, which holds the whole numbering: the view keeps only `numbers`.
      const make = noteViewRebuild(scope, options, numbers, shown, lost);
      return view({ source: shown, input: key, streaming, note: id }, make(), make);
    });
  };
  const notesOf = (
    numbering: Numbering,
    streamingNotes: Footnotes,
    lost: ReadonlySet<string>,
  ): BlockView | undefined => {
    const items: BlockView[] = [];
    for (const id of numbering.order) {
      const note = numbering.notes.get(id);
      if (note) items.push(noteViewOf(id, numbering, streamingNotes.notes.includes(note), lost));
    }
    if (items.length === 0) return (lastNotes = undefined);
    const before = lastNotes?.items;
    if (before?.length === items.length && items.every((item, index) => item === before[index]))
      return lastNotes;
    const source = items.map((item) => item.source).join("");
    const input = items.map((item) => item.input).join("\u0001");
    const fields: Fields = { source, input, streaming: false, notes: true, items };
    return (lastNotes = view(fields, EMPTY_HAST, () => EMPTY_HAST));
  };

  /** The block views with every heading's id numbered across the answer (slugs.ts). */
  const slugId = (value: string) => CLOBBER_PREFIX + headingId(scope, value);
  const numberedSlugs = createSlugNumbering<BlockView>((base, slugs) =>
    variant(base, {}, (hast) => withSlugs(hast, slugs, slugId)),
  );

  /** A view as a block that continues the paragraph before (BlockView's `continues`), the same object each time. */
  const continuingOf = createVariants((base: BlockView) => variant(base, { continues: true }));
  /** A paragraph's parts joined (joinParts), the same string while its parts are the same strings. */
  const joinOf = createJoiner();

  let lastText: string | undefined;
  let lastStreaming = true;
  let lastViews: readonly BlockView[] = [];
  /** The block views before numberedSlugs: what a finished block keeps from one update to the next. */
  let lastBlockViews: readonly BlockView[] = [];
  let lastBlocks: readonly string[] = [];
  let lastEntries: readonly Entry[] = [];
  let lastSignatures: readonly string[] = [];
  let lastLinesUsed: DefinitionLines | undefined;
  let lastDefinedUsed: ReadonlySet<string> | undefined;
  return {
    update(raw, streaming) {
      if (!entities && characterEntitiesLoaded()) {
        // The full table arrived: blocks holding `&` are parsed again, the rest keep their views.
        entities = true;
        appender.forget(); // and no tail parse from before it is extended
        if (missed) {
          for (const cache of [trees, owned, facts, repairs, views, tails]) cache.forget("&");
          lastLinesUsed = undefined;
          lastText = undefined;
        }
      }
      if (raw === lastText && streaming === lastStreaming) return lastViews;
      // The finished render parses afresh what the fast path made (append.ts), and what was read from it.
      if (!streaming)
        for (const made of appender.made()) for (const cache of [trees, facts]) cache.forget(made);
      const text = capNesting(raw.includes("\r") ? raw.replace(/\r\n?/g, "\n") : raw);
      let blocks: readonly string[];
      let continues: readonly boolean[];
      try {
        blocks = split(text);
        continues = split.continues();
        // A finished paragraph's parts are one block again, parsed whole (split.ts's MAX_JOINED).
        ({ blocks, continues } = joinParts(blocks, continues, streaming, joinOf));
      } catch {
        blocks = [text];
        continues = [false];
      }
      const last = blocks.length - 1;
      // No append fast path within a paragraph split.ts cuts, nor one long enough to be cut.
      appender.open(streaming && !continues[last] && (blocks[last]?.length ?? 0) <= MAX_PARAGRAPH);
      const lines = documentOf(blocks, streaming ? last : blocks.length);
      const defined = documentNotesOf(blocks);
      // A block finished last time, unchanged, under the same definitions: its entry as it was.
      const reusable = lines === lastLinesUsed ? lastBlocks.length - (lastStreaming ? 1 : 0) : 0;
      const entries = blocks.map((block, index): Entry => {
        const before = lastEntries[index];
        if (streaming && index === last) {
          inTail = true;
          try {
            return tailEntryOf(block, lines, defined);
          } finally {
            inTail = false;
          }
        }
        if (
          before &&
          index < reusable &&
          block === lastBlocks[index] &&
          (!before.capped || defined === lastDefinedUsed)
        ) {
          for (const cache of [noteLabels, defines, labels, inputs]) cache.keep(block);
          facts.keep(before.input);
          return before;
        }
        return entryOf(block, lines, defined, streaming);
      });
      const numbering = numberFootnotes(
        entries.map((entry) => entry.footnotes),
        streaming,
      );
      const previous = lastBlockViews;
      const blockViews = entries.map((entry, index) => {
        const { signature, nths } = numbering.blocks[index]!;
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
        const references = () => referencesFor(numbering, entry.footnotes.refs);
        if (!tail)
          return views.get(entry.key, () => toView(entry.source, viewInput, references(), nths));
        // The streaming tail's view holds the block as written (for its fallback), not the repaired text,
        // and so is cached on both.
        const written = blocks[index]!;
        return tails.get(`${entry.key}\u0003${written}`, () => {
          inTail = true;
          try {
            return toView(entry.source, viewInput, references(), nths, written);
          } finally {
            inTail = false;
          }
        });
      });
      // An empty answer (every streamed answer starts as one) has no blocks, so no tail.
      const tail = streaming ? entries[last] : undefined;
      const lost = new Set(blockViews.flatMap((view) => view.lostReferences ?? []));
      const notes = notesOf(numbering, tail ? tail.footnotes : NO_FOOTNOTES, lost);
      const anchored = numberedSlugs(
        blockViews.map((view, index) => (continues[index] ? continuingOf(view) : view)),
      );
      lastViews = notes ? [...anchored, notes] : anchored;
      lastBlockViews = blockViews;
      lastBlocks = blocks;
      lastEntries = entries;
      lastSignatures = numbering.blocks.map((block) => block.signature);
      lastLinesUsed = lines;
      lastDefinedUsed = defined;
      lastText = raw;
      lastStreaming = streaming;
      for (const cache of caches) cache.sweep();
      trees.retain(tailKeys);
      tailKeys.clear();
      appender.end(streaming);
      joinOf.sweep();
      // Every view this update made lets its hast go; those it shows make their React elements first.
      // A stream makes a view or two an update, near the end: looked for from there.
      const shownViews = notes?.items ? [...lastViews, ...notes.items] : lastViews;
      const shown =
        made.length > 8
          ? (
              (all) => (view: BlockView) =>
                all.has(view)
            )(new Set(shownViews))
          : (view: BlockView) => shownViews.lastIndexOf(view) >= 0;
      for (const one of made) seal(one, shown(one));
      made.length = 0;
      return lastViews;
    },
    waitsForEntities: () => missed && !entities,
  };
}
