// The script of a piece of text, for its lang attribute: text whose letters are mostly Devanagari is Hindi, so it
// takes the Hindi type settings (tokens-type.css: the Devanagari face, taller lines, no tracking) in either
// interface language. The answer's Markdown marks its blocks with it (markdown/to-react.ts).

const LETTER = /\p{L}/u;
const MARK = /\p{M}/u;

/** What a code point is: 0 not a letter, 1 a letter of another script, 2 a Devanagari letter, 3 a mark. */
type Kind = 0 | 1 | 2 | 3;

/**
 * Devanagari (U+0900 to U+097F) by its table, without a regex: the commonest code points after ASCII in a Hindi
 * answer. Every one is a letter or a mark but the dandas, the digits and the abbreviation sign (U+0964 to U+0970).
 */
function devanagari(code: number): Kind {
  if (code >= 0x0964 && code <= 0x0970) return 0;
  if (
    code <= 0x0903 ||
    (code >= 0x093a && code <= 0x093c) ||
    (code >= 0x093e && code <= 0x094f) ||
    (code >= 0x0951 && code <= 0x0957) ||
    code === 0x0962 ||
    code === 0x0963
  )
    return 3;
  return 2;
}

function kindOf(code: number, char: string): Kind {
  if (code < 0x80) {
    const lower = code | 0x20;
    return lower >= 0x61 && lower <= 0x7a ? 1 : 0;
  }
  if (code >= 0x0900 && code <= 0x097f) return devanagari(code);
  if (LETTER.test(char)) return code >= 0xa8e0 && code <= 0xa8ff ? 2 : 1;
  return MARK.test(char) ? 3 : 0;
}

/**
 * "hi" when Devanagari letters (U+0900 to U+097F and Devanagari Extended, U+A8E0 to U+A8FF) are more than half of
 * the text's letters, otherwise undefined (no letter at all, digits only and an empty string included). A mark
 * counts as a letter of the letter it sits on, whatever block the mark is in; one with no letter before it counts
 * for nothing. Devanagari's vowel signs and viramas so weigh it about 1.6 times its letters in the vote. It walks the text by code point and tests each one alone: no end-anchored class, which V8 14.6 gets
 * wrong for astral letters (#248).
 */
export function langOfText(text: string): "hi" | undefined {
  let deva = 0;
  let letters = 0;
  // the kind of the letter a mark would sit on: 0 none (or something else since), 1 or 2 as kindOf's
  let base: Kind = 0;
  for (const char of text) {
    const kind = kindOf(char.codePointAt(0)!, char);
    if (kind !== 3) base = kind;
    if (base === 0) continue;
    letters++;
    if (base === 2) deva++;
  }
  return deva * 2 > letters ? "hi" : undefined;
}
