// A long paste as a chip on the composer's well, not words in its field: the field stays small, so each keystroke
// stays cheap however much was pasted (a 6 MiB paste in the field took over a second a key in headless Chromium).
// The composer's words are still one string to everything outside it: the typed words, then each chip's text in paste
// order, a blank line between (askedOf). A string coming back (a draft, a stopped question, a lost edit) is split
// into chips again by the chip texts remembered here (splitAsked); a long text with no chip remembered becomes one.

/** A paste of this many UTF-16 units or more (about eight pages) is a chip. Measured on the probe in headless
 * Chromium: with 20,000 characters in the field no keystroke reached Event Timing's 16 ms floor. */
export const PASTE_CHIP_MIN = 20_000;

/** Between the typed words and each chip's text, as they are sent. */
export const CHIP_GAP = "\n\n";

/** One pasted text, as its chip holds it: its words counted once, as it comes. */
export type PasteChip = { id: number; text: string; words: number };

let next = 1;

/** The words in `text`: runs of anything but whitespace. One pass, no copy. */
export function wordCount(text: string): number {
  let words = 0;
  let inWord = false;
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i);
    // ASCII whitespace, NBSP, the Unicode spaces and line separators, the BOM
    const space =
      c === 32 ||
      (c >= 9 && c <= 13) ||
      c === 0xa0 ||
      c === 0x1680 ||
      (c >= 0x2000 && c <= 0x200a) ||
      c === 0x2028 ||
      c === 0x2029 ||
      c === 0x202f ||
      c === 0x205f ||
      c === 0x3000 ||
      c === 0xfeff;
    if (space) inWord = false;
    else if (!inWord) {
      inWord = true;
      words += 1;
    }
  }
  return words;
}

/** The chip texts seen lately, newest last: what splitAsked looks for. Bounded, as each holds a long text. */
const REMEMBERED: PasteChip[] = [];
const REMEMBER_MAX = 32;

/** A new chip for `text`, remembered so the string it goes out in can be split again. */
export function chipOf(text: string): PasteChip {
  const known = REMEMBERED.find((c) => c.text === text);
  const chip = { id: next++, text, words: known?.words ?? wordCount(text) };
  if (known === undefined) {
    REMEMBERED.push({ ...chip });
    if (REMEMBERED.length > REMEMBER_MAX) REMEMBERED.shift();
  }
  return chip;
}

/** Forgets every remembered chip text. For tests only. */
export function forgetChipsForTests(): void {
  REMEMBERED.length = 0;
}

/** The chips' texts joined as they follow the typed words, made once per change of the chips. */
export const chipTail = (chips: readonly PasteChip[]): string =>
  chips.map((c) => c.text).join(CHIP_GAP);

/** What the composer holds as one string: the typed words, then the chips' texts (chipTail), a blank line between.
 * With no typed words, the chips alone. A concatenation, so the engine makes it without copying the long tail. */
export function askedOf(typed: string, tail: string): string {
  if (tail === "") return typed;
  return typed === "" ? tail : typed + CHIP_GAP + tail;
}

/** `text` split back into typed words and chips: each remembered chip text found in it, in the order they stand,
 * with the blank line before it (or after it, at the start); what is left is the typed words. Left words of
 * PASTE_CHIP_MIN units or more, such as a long question whose chip is no longer remembered, become a chip of their own
 * at the front, so the field never takes a long text back. */
export function splitAsked(text: string): { typed: string; chips: PasteChip[] } {
  if (text.length < PASTE_CHIP_MIN) return { typed: text, chips: [] };
  const spans: { start: number; end: number; chip: PasteChip }[] = [];
  const free = (s: number, e: number) => spans.every((x) => e <= x.start || s >= x.end);
  for (let k = REMEMBERED.length - 1; k >= 0; k--) {
    const known = REMEMBERED[k]!;
    for (let at = text.indexOf(known.text); at !== -1; at = text.indexOf(known.text, at + 1)) {
      const end = at + known.text.length;
      if (free(at, end)) spans.push({ start: at, end, chip: known });
    }
  }
  spans.sort((a, b) => a.start - b.start);
  let typed = "";
  let from = 0;
  for (const { start, end } of spans) {
    let s = start;
    let e = end;
    if (s - CHIP_GAP.length >= from && text.startsWith(CHIP_GAP, s - CHIP_GAP.length))
      s -= CHIP_GAP.length;
    else if (text.startsWith(CHIP_GAP, e)) e += CHIP_GAP.length;
    typed += text.slice(from, s);
    from = e;
  }
  typed += text.slice(from);
  const chips = spans.map(({ chip }) => ({ ...chip, id: next++ }));
  if (typed.length >= PASTE_CHIP_MIN) return { typed: "", chips: [chipOf(typed), ...chips] };
  return { typed, chips };
}
