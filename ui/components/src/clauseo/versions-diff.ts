// The blackline. Two revisions of a document, word by word: what stayed, what came in and what went out. Words
// between {{ and }} are the lawyer's own and keep the lawyer's weight wherever they stand. The caller passes the
// revisions' words. Pure: no DOM, no React.

/** A run of words: the same in both, added in the later, or taken out of it. mine: the lawyer wrote it. */
export type Run = { kind: "same" | "ins" | "del"; text: string; mine: boolean };
type Word = { text: string; mine: boolean };

/** A word with what joins it inside, "P-2", "12(3)", "A.B", or one mark of punctuation, each with its trailing
 * space: so "notice." against "notice, at" changes only what came between. */
const TOKEN =
  /[\p{L}\p{N}\p{M}]+(?:[.\-'’(][\p{L}\p{N}\p{M}]+)*\)?\s*|[^\s\p{L}\p{N}\p{M}]\s*|\s+/gu;

/** A paragraph's words, each keeping its own trailing space, and whether the lawyer wrote it. */
export function words(s: string): Word[] {
  const out: Word[] = [];
  const parts = s.split(/(\{\{|\}\})/);
  let mine = false;
  for (const part of parts) {
    if (part === "{{") mine = true;
    else if (part === "}}") mine = false;
    else
      for (const m of part.matchAll(TOKEN)) {
        const last = out[out.length - 1];
        // a space alone joins the word before it, so it never stands as a change of its own
        if (m[0].trim() === "" && last !== undefined) last.text += m[0];
        else out.push({ text: m[0], mine });
      }
  }
  return out;
}

/** The text without its marks, for a screen reader and a count. */
export const plain = (s: string) => s.replace(/\{\{|\}\}/g, "");

/** Joins neighbouring words of one kind and one hand into runs. */
function merge(list: Run[]): Run[] {
  const out: Run[] = [];
  for (const r of list) {
    const last = out[out.length - 1];
    if (last !== undefined && last.kind === r.kind && last.mine === r.mine) last.text += r.text;
    else out.push({ ...r });
  }
  return out;
}

/** a against b, word by word, by their longest common run of words. */
export function diffWords(a: string, b: string): Run[] {
  const x = words(a);
  const y = words(b);
  const key = (w: Word) => w.text.trim();
  const n = x.length;
  const m = y.length;
  // lcs[i][j]: the longest common run of x[i..] and y[j..]
  const lcs: number[][] = Array.from({ length: n + 1 }, () =>
    Array.from({ length: m + 1 }, () => 0),
  );
  for (let i = n - 1; i >= 0; i--) {
    const row = lcs[i] as number[];
    const below = lcs[i + 1] as number[];
    for (let j = m - 1; j >= 0; j--) {
      row[j] =
        key(x[i] as Word) === key(y[j] as Word)
          ? (below[j + 1] ?? 0) + 1
          : Math.max(below[j] ?? 0, row[j + 1] ?? 0);
    }
  }
  const out: Run[] = [];
  let i = 0;
  let j = 0;
  while (i < n || j < m) {
    const xi = x[i];
    const yj = y[j];
    if (xi !== undefined && yj !== undefined && key(xi) === key(yj)) {
      out.push({ kind: "same", text: yj.text, mine: yj.mine });
      i++;
      j++;
    } else if (
      xi !== undefined &&
      (yj === undefined || (lcs[i + 1]?.[j] ?? 0) >= (lcs[i]?.[j + 1] ?? 0))
    ) {
      // what went out reads before what came in, as a blackline sets it
      out.push({ kind: "del", text: xi.text, mine: xi.mine });
      i++;
    } else if (yj !== undefined) {
      out.push({ kind: "ins", text: yj.text, mine: yj.mine });
      j++;
    }
  }
  return merge(out);
}

/** One paragraph place of a document, compared: the same, changed, added or taken out. */
export type ParaDiff = {
  place: number;
  kind: "same" | "changed" | "added" | "removed";
  runs: Run[];
};

const asRuns = (kind: Run["kind"], s: string): Run[] =>
  merge(words(s).map((w) => ({ kind, text: w.text, mine: w.mine })));

/** A document's paragraphs from one revision to another, place by place. Each place holds a paragraph's words or
 * nothing; a revision not made yet is an empty list. */
export function diffParagraphs(
  from: readonly (string | null)[],
  to: readonly (string | null)[],
): ParaDiff[] {
  const out: ParaDiff[] = [];
  const places = Math.max(from.length, to.length);
  for (let p = 0; p < places; p++) {
    const x = from[p] ?? null;
    const y = to[p] ?? null;
    if (x === null && y !== null) out.push({ place: p, kind: "added", runs: asRuns("ins", y) });
    else if (y === null && x !== null)
      out.push({ place: p, kind: "removed", runs: asRuns("del", x) });
    else if (x !== null && y !== null) {
      out.push(
        x === y
          ? { place: p, kind: "same", runs: asRuns("same", y) }
          : { place: p, kind: "changed", runs: diffWords(x, y) },
      );
    }
  }
  return out;
}

/** One entry of a list of dates: its date and its event, in words. */
export type DateEntry = readonly [date: string, event: string];

/** One row of a list of dates, compared: its date and its event as runs. */
export type RowDiff = { row: string; kind: ParaDiff["kind"]; date: Run[]; event: Run[] };

/** A list of dates from one revision to another, row by row in the order rows gives. Each revision holds an entry
 * for the rows it has. */
export function diffRows(
  rows: readonly string[],
  from: Readonly<Partial<Record<string, DateEntry>>>,
  to: Readonly<Partial<Record<string, DateEntry>>>,
): RowDiff[] {
  const out: RowDiff[] = [];
  for (const row of rows) {
    const x = from[row];
    const y = to[row];
    if (x === undefined && y !== undefined) {
      out.push({ row, kind: "added", date: asRuns("ins", y[0]), event: asRuns("ins", y[1]) });
    } else if (y === undefined && x !== undefined) {
      out.push({ row, kind: "removed", date: asRuns("del", x[0]), event: asRuns("del", x[1]) });
    } else if (x !== undefined && y !== undefined) {
      out.push(
        x[0] === y[0] && x[1] === y[1]
          ? { row, kind: "same", date: asRuns("same", y[0]), event: asRuns("same", y[1]) }
          : { row, kind: "changed", date: diffWords(x[0], y[0]), event: diffWords(x[1], y[1]) },
      );
    }
  }
  return out;
}

/** Whether any place changed. */
export const anyChange = (list: readonly { kind: string }[]) => list.some((d) => d.kind !== "same");
