// A new conversation's title, taken from its first message (HANDOFF item 9): its words with the whitespace collapsed,
// whole when 48 characters or fewer, else its first 6 words. Characters are code points, as the host counts them
// (`MAX_TITLE_CHARS`, crates/protocol/src/title.rs), so a Hindi title is measured as the host will measure it. A title
// the host would refuse (`check_display_title`: too long, nothing that shows, a control, bidi control or line
// separator) is no title: the room sends no rename and the conversation stays "New conversation".

/** A message this long or shorter, in code points, is its own title. */
export const WHOLE_TITLE_CHARS = 48;
/** A longer message's title is its first this many words. */
export const TITLE_WORDS = 6;
/** The host's limit on a title, in code points: `MAX_TITLE_CHARS`. */
export const MAX_TITLE_CHARS = 200;

/** Code points, as Rust's `chars()` counts them: not graphemes, so an emoji with a modifier counts as two, as the host
 * counts it. */
const codePoints = (text: string): string[] => Array.from(text);

const chars = (text: string): number => codePoints(text).length;

/** Code point ranges, inclusive. */
type Ranges = readonly (readonly [number, number])[];

const within = (ranges: Ranges, c: number) => ranges.some(([lo, hi]) => c >= lo && c <= hi);

/** What `check_display_title` refuses anywhere in a title: C0, DEL and C1 (Rust's `char::is_control`), the bidi
 * embeddings, overrides and isolates, and the line and paragraph separators. */
const REFUSED: Ranges = [
  [0x00, 0x1f],
  [0x7f, 0x9f],
  [0x202a, 0x202e],
  [0x2066, 0x2069],
  [0x2028, 0x2029],
];

/** What `check_display_title` counts as showing nothing, beside whitespace: the default-ignorable code points it
 * lists, U+2800, and the combining marks it lists, which show only on a character before them. */
const INVISIBLE: Ranges = [
  [0x00ad, 0x00ad],
  [0x034f, 0x034f],
  [0x061c, 0x061c],
  [0x115f, 0x1160],
  [0x17b4, 0x17b5],
  [0x180b, 0x180f],
  [0x200b, 0x200f],
  [0x202a, 0x202e],
  [0x2060, 0x206f],
  [0x2800, 0x2800],
  [0x3164, 0x3164],
  [0xfe00, 0xfe0f],
  [0xfeff, 0xfeff],
  [0xffa0, 0xffa0],
  [0xfff0, 0xfff8],
  [0x1bca0, 0x1bca3],
  [0x1d173, 0x1d17a],
  [0xe0000, 0xe0fff],
  // the combining marks
  [0x0300, 0x036f],
  [0x0483, 0x0489],
  [0x0591, 0x05bd],
  [0x05bf, 0x05bf],
  [0x05c1, 0x05c2],
  [0x05c4, 0x05c5],
  [0x05c7, 0x05c7],
  [0x0610, 0x061a],
  [0x064b, 0x065f],
  [0x0670, 0x0670],
  [0x06d6, 0x06dc],
  [0x06df, 0x06e4],
  [0x06e7, 0x06e8],
  [0x06ea, 0x06ed],
  [0x0e31, 0x0e31],
  [0x0e34, 0x0e3a],
  [0x0e47, 0x0e4e],
  [0x1ab0, 0x1aff],
  [0x1dc0, 0x1dff],
  [0x20d0, 0x20ff],
  [0xfe20, 0xfe2f],
];

const WHITESPACE = /^\s$/u;

/** Whether the host would take `title` as a thread's title: `check_display_title`'s rules. */
export function isTitle(title: string): boolean {
  const points = codePoints(title);
  if (points.length > MAX_TITLE_CHARS) return false;
  const codes = points.map((p) => p.codePointAt(0) ?? 0);
  if (codes.some((c) => within(REFUSED, c))) return false;
  return points.some((p, i) => !WHITESPACE.test(p) && !within(INVISIBLE, codes[i] ?? 0));
}

/** The title a first message gives its conversation, or null when it gives none the host would take. */
export function titleOf(message: string): string | null {
  const words = message.split(/\s+/u).filter((word) => word !== "");
  const whole = words.join(" ");
  const title = chars(whole) <= WHOLE_TITLE_CHARS ? whole : words.slice(0, TITLE_WORDS).join(" ");
  return isTitle(title) ? title : null;
}
