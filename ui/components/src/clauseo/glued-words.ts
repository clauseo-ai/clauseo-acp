// Where glued.tsx cuts words to hold an icon to them: the last word (or the first), a long one's few letters.

/** A last word longer than this keeps only its last few letters with the icon, so a long file name with no spaces
 * can still break and never pushes past its line. */
const LONG = 16;
const KEEP = 6;

/** The letters of `s`, whole graphemes where the platform says what one is, so a cut never splits a letter from its
 * marks. */
function letters(s: string): string[] {
  if (typeof Intl !== "undefined" && "Segmenter" in Intl)
    return [...new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(s)].map(
      (g) => g.segment,
    );
  return Array.from(s);
}

/** `s` split as the free words and the end the icon keeps: the last word, or a long one's last letters. */
export function tailOf(s: string): [string, string] {
  const at = s.trimEnd().search(/\S+$/);
  if (at < 0) return [s, ""];
  const word = s.slice(at);
  const ls = letters(word);
  if (ls.length <= LONG) return [s.slice(0, at), word];
  const keep = ls.slice(-KEEP).join("");
  return [s.slice(0, s.length - keep.length), keep];
}

/** `s` split as the start the icon keeps (the first word, or a long one's first letters) and the free words. */
export function headOf(s: string): [string, string] {
  const m = /^\S+/.exec(s);
  if (m === null) return ["", s];
  const ls = letters(m[0]);
  if (ls.length <= LONG) return [m[0], s.slice(m[0].length)];
  const keep = ls.slice(0, KEEP).join("");
  return [keep, s.slice(keep.length)];
}
