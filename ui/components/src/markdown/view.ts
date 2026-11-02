// One block's view (answer.ts makes them), and what it keeps (#233).
// What a view keeps (#233): its React elements, its source and input, and the footnote data the
// document-wide passes read (each block's Footnotes, with its notes' mdast). Its hast goes when the update
// that made it ends, once its elements are made, and no block's mdast outlives the update but the streaming
// last block's. What needs a finished block parsed again parses its kept input: a note arriving for one of
// its references, a link definition it uses arriving or changing, the entity table arriving after it missed a
// name, and the final render when that changes its input (each a new cache key, so a new view).
import type { Root as Hast, Text } from "hast";
import type { ReactNode } from "react";
import { jsx } from "react/jsx-runtime";
import { toReact } from "./to-react.ts";

/**
 * One block, ready to render: `source` is the block's text, `input` what was parsed for it (with its
 * footnote numbers, for the notes view a key), `content` its React elements. `streaming` marks the last
 * block of a streaming answer, whose links stay inert until it is finished (a URL may still be arriving,
 * and tail repair may have rewritten it). `notes` marks the answer's notes, after its last block: `items`
 * holds one view per note, in number order, each with its note's identifier as `note`, and its `hast` is
 * empty (the section around the items is the renderer's).
 */
export type BlockView = {
  readonly source: string;
  readonly input: string;
  /**
   * The sanitised hast. The view holds it only until the update that made it ends: then it keeps its
   * React elements (`content`) instead, and reading `hast` makes it again from the view's input (a
   * parse; the tests read it, the renderer does not).
   */
  readonly hast: Hast;
  /** Its React elements (to-react.ts), made from its hast as the update that made the view ends. */
  readonly content: ReactNode;
  readonly streaming: boolean;
  readonly notes?: true;
  readonly items?: readonly BlockView[];
  readonly note?: string;
  /** The slug of each of its headings, in order, before the answer's repeats are numbered. */
  readonly slugs?: readonly string[];
  /**
   * The block continues the paragraph of the block before: a paragraph longer than split.ts's
   * MAX_PARAGRAPH is cut into parts while it streams (and for good, past MAX_JOINED). Its first paragraph
   * is the rest of that one, rendered without the gap between paragraphs (to-react.ts).
   */
  readonly continues?: true;
  /**
   * The ids of the footnote references this block's view shows as plain text, inside a dropped
   * never-markup pair (inline-tags.ts): their notes leave out the back-reference to them.
   */
  readonly lostReferences?: readonly string[];
};

export const EMPTY_HAST: Hast = { type: "root", children: [] };

/** A view's own fields; its hast and elements are held apart (Held). */
export type Fields = Omit<BlockView, "hast" | "content">;

/**
 * What a view holds besides its fields: its hast until it is sealed, its React elements once made, and how
 * to make its hast again (from its input, with what its footnote references read of the numbering).
 */
type Held = {
  hast: Hast | undefined;
  content: ReactNode;
  made: boolean;
  readonly rebuild: () => Hast;
};

const HELD = new WeakMap<object, Held>();

/** Throws `error` when rendered: an error making a block's elements, raised inside its error boundary. */
function Rethrow({ error }: { error: unknown }): never {
  throw error;
}

/**
 * A view's React elements. Made by the model, but an error on the way is thrown where rendering the block
 * would have thrown it, so the block's error boundary (boundary.tsx) shows its source as text, as before.
 */
function contentOf(view: BlockView, held: Held): ReactNode {
  try {
    return toReact(held.hast ?? held.rebuild(), view.continues);
  } catch (error) {
    return jsx(Rethrow, { error });
  }
}

/** The accessors every view shares: `hast` (held, or made again) and `content` (made once). */
const ACCESSORS: PropertyDescriptorMap = {
  hast: {
    enumerable: true,
    get(this: object) {
      const held = HELD.get(this)!;
      return held.hast ?? held.rebuild();
    },
  },
  content: {
    enumerable: true,
    get(this: BlockView) {
      const held = HELD.get(this)!;
      if (!held.made) {
        held.content = contentOf(this, held);
        held.made = true;
      }
      return held.content;
    },
  },
};

/** A view of `fields`, holding `hast` (if given) until sealed and made again by `rebuild` after. */
export function viewOf(fields: Fields, hast: Hast | undefined, rebuild: () => Hast): BlockView {
  const view = Object.defineProperties({ ...fields }, ACCESSORS) as BlockView;
  HELD.set(view, { hast, content: undefined, made: false, rebuild });
  return view;
}

/**
 * `base` with `extra` fields and its hast through `change`: a variant (a continuing block, renumbered
 * headings) that holds its hast as `base` does, and makes it again from `base`'s.
 */
export function variantOf(
  base: BlockView,
  extra: Partial<Fields>,
  change: (hast: Hast) => Hast = (hast) => hast,
): BlockView {
  const held = HELD.get(base)!;
  const hast = held.hast === undefined ? undefined : change(held.hast);
  // Its own fields, not its accessors (a spread would read them).
  const fields: Record<string, unknown> = {};
  for (const key of Object.keys(base))
    if (key !== "hast" && key !== "content") fields[key] = base[key as keyof Fields];
  return viewOf({ ...(fields as Fields), ...extra }, hast, () => change(base.hast));
}

/**
 * Lets a view's hast go, as the update that made it ends: a view the update shows (`shown`) makes its
 * React elements first; one it does not (a block shown through a variant) makes them only if ever read.
 */
export function seal(view: BlockView, shown: boolean): void {
  const held = HELD.get(view);
  if (held?.hast === undefined) return;
  if (shown && !held.made) {
    held.content = contentOf(view, held);
    held.made = true;
  }
  held.hast = undefined;
}

/** Whether `view` still holds its hast (for the tests: no view does once its update has ended). */
export function holdsHast(view: BlockView): boolean {
  return HELD.get(view)?.hast !== undefined;
}

/** A block shown as its source, in plain text: what a block that cannot render safely falls back to. */
export function plainText(source: string): Hast {
  const text: Text = { type: "text", value: source.trimEnd() };
  return {
    type: "root",
    children: [{ type: "element", tagName: "p", properties: {}, children: [text] }],
  };
}
