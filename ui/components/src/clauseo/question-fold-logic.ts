// What the folded question (question-fold.tsx) and the pinned question (thread.tsx) draw of a long question: its
// start, so neither lays out more than a few lines of a 6 MiB question.

/** The lines a folded question shows. */
export const FOLD_LINES = 8;

/** Past this many UTF-16 units a question is past FOLD_LINES lines at any width and text size: the narrowest
 * question column (about 340 px) at the largest text size (20) holds under 40 Latin characters a line, so 2,000
 * fill 50 lines; a Devanagari line holds more units, but not six times as many. */
export const FOLD_CHARS = 2_000;

/** The start of `text`, at most `n` UTF-16 units, never ending inside a surrogate pair. */
export function startOf(text: string, n: number): string {
  if (text.length <= n) return text;
  const code = text.charCodeAt(n - 1);
  return text.slice(0, code >= 0xd800 && code <= 0xdbff ? n - 1 : n);
}
