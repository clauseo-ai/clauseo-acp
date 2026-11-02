// One note's hast for the notes view (answer.ts): the note rendered with its numbers (footnotes.ts), its
// cut tags and stand-ins put back, and sanitised. A note's view keeps its NoteNumbers and makes this again
// from them when its hast is read after its update has ended (view.ts).
import type { Root as Hast, Text } from "hast";
import { sanitize } from "hast-util-sanitize";
import type { Options } from "mdast-util-to-hast";
import { dropBackReferences, noteHast } from "./footnotes.ts";
import type { NoteNumbers } from "./footnotes.ts";
import { markCutTags, pairTags } from "./inline-tags.ts";
import { deeperThan, hasStandIns, MAX_TREE_DEPTH, restoreText } from "./nesting.ts";
import { SCHEMA } from "./schema.ts";

/**
 * A note's hast. As answer.ts's toView does for a block: a note too deep to walk safely, or any error on the
 * way, shows its source (`shown`) as plain text, and what capNesting replaced reads as written. `lost` are
 * the references whose back-references it leaves out (dropBackReferences).
 */
export function noteViewHast(
  scope: string,
  options: Options,
  numbers: NoteNumbers,
  shown: string,
  lost: ReadonlySet<string>,
): Hast {
  const { note } = numbers;
  try {
    if (deeperThan(note.node, MAX_TREE_DEPTH)) throw new Error("too deep");
    const raw = noteHast(scope, numbers, options);
    // The note's own text, and the link definitions from other blocks it carries, may hold some.
    markCutTags(raw, note.source, note.node.position?.start.offset ?? 0);
    if (hasStandIns(note.source) || note.definitions.length > 0) restoreText(raw);
    pairTags(raw, false);
    if (lost.size > 0) dropBackReferences(raw, lost);
    return sanitize(raw, SCHEMA) as Hast;
  } catch {
    const text: Text = { type: "text", value: shown.trimEnd() };
    return {
      type: "root",
      children: [{ type: "element", tagName: "li", properties: {}, children: [text] }],
    };
  }
}

/**
 * How a note's view makes its hast (noteViewHast), now and again after its update has ended. A closure of its
 * own, holding only these: one made inside answer.ts's noteViewOf would keep that update's whole numbering.
 */
export function noteViewRebuild(
  scope: string,
  options: Options,
  numbers: NoteNumbers,
  shown: string,
  lost: ReadonlySet<string>,
): () => Hast {
  return () => noteViewHast(scope, options, numbers, shown, lost);
}
