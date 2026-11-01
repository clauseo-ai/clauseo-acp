// The edits tail repair (repair.ts) makes to a block's text: cuts, kept as a mask so whether a character
// stays is one lookup, and applying them with an insertion, noting where each stretch of the result came
// from so an offset can be traced back to the input.

const WHITESPACE = /\s/u;

export type Edit = { start: number; end: number; text: string };

/**
 * The parts of a text a pass cuts: the edits, and a mask of the characters they remove, so whether a
 * character stays is one lookup however many cuts there are (a hostile `[`×16,000 cuts every one).
 */
export type Cuts = {
  readonly edits: Edit[];
  readonly mask: Uint8Array;
  readonly cut: (start: number, end: number) => void;
  readonly kept: (at: number) => boolean;
};

/** New cuts of a text of `length`; with `base`, what `base` cuts counts as cut too (but is not in `edits`). */
export function createCuts(length: number, base?: Cuts): Cuts {
  const edits: Edit[] = [];
  const mask = base ? base.mask.slice() : new Uint8Array(length);
  return {
    edits,
    mask,
    cut: (start, end) => {
      edits.push({ start, end, text: "" });
      mask.fill(1, start, end);
    },
    kept: (at) => mask[at] !== 1,
  };
}

/** The end of what `cuts` keep of `input`, before trailing whitespace. */
export function keptEnd(input: string, from: number, cuts: Cuts): number {
  let at = input.length;
  while (at > from && (!cuts.kept(at - 1) || WHITESPACE.test(input[at - 1]!))) at--;
  return at;
}

/** Where a stretch of an output came from: `length` characters at `out`, copied from `at` in the input. */
export type Kept = { out: number; at: number; length: number };

/**
 * `input` with `edits` applied and `insert` placed at `end`. Each stretch copied from `input` is noted
 * in `kept`, if given, so an offset of the result can be traced back (originOf).
 */
export function apply(
  input: string,
  edits: Edit[],
  end: number,
  insert: string,
  kept?: Kept[],
): string {
  edits.sort((a, b) => a.start - b.start);
  let out = "";
  const copy = (from: number, to: number) => {
    if (to <= from) return;
    kept?.push({ out: out.length, at: from, length: to - from });
    out += input.slice(from, to);
  };
  let at = 0;
  let inserted = insert === "";
  for (const edit of edits) {
    if (edit.end <= at) continue;
    const start = Math.max(at, edit.start);
    if (!inserted && end <= start) {
      copy(at, end);
      out += insert;
      at = end;
      inserted = true;
    }
    copy(at, start);
    out += edit.text;
    at = edit.end;
  }
  if (!inserted) {
    copy(at, Math.max(at, end));
    out += insert;
    at = Math.max(at, end);
  }
  copy(at, input.length);
  return out;
}

/** The input offset an output offset was copied from (apply's `kept`), or -1 for inserted text. */
export function originOf(kept: readonly Kept[], offset: number): number {
  for (const stretch of kept)
    if (offset >= stretch.out && offset < stretch.out + stretch.length)
      return stretch.at + offset - stretch.out;
  return -1;
}
