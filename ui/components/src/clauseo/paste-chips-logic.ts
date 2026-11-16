// A long paste as a chip on the composer's well, not words in its field: the field stays small, so each keystroke
// stays cheap however much was pasted (a 6 MiB paste in the field took over a second a key in headless Chromium).
// What the composer holds is a Draft: the typed words, and each chip's text in paste order, kept apart. It is joined
// into one string only to go (sentOf): the chips' texts first, in paste order, as they are drawn above the field, then
// the typed words, a blank line between. A draft is never split again by searching its joined string: it comes back as
// the Draft it was. A sent string coming back from the host (a stopped question, a steer not sent, a lost edit) is
// looked up whole among the questions this app sent (draftOf); a long one not found is one chip, as it stands.

/** A paste of this many UTF-16 units or more (about eight pages) is a chip. Measured on the probe in headless
 * Chromium: with 20,000 characters in the field no keystroke reached Event Timing's 16 ms floor. */
export const PASTE_CHIP_MIN = 20_000;

/** Between each chip's text and the next, and before the typed words, as they are sent. */
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

/** A new chip for `text`, its words counted once. */
export function chipOf(text: string): PasteChip {
  return { id: next++, text, words: wordCount(text) };
}

/** What the composer holds, and what a conversation's draft keeps: the words typed in the field, and each long paste's
 * text in paste order. Kept apart, never joined and split again, so a chip whose text holds another's comes back
 * byte for byte. */
export type Draft = { typed: string; pastes: readonly string[] };

export const EMPTY_DRAFT: Draft = { typed: "", pastes: [] };

/** A draft with nothing in it: no typed character and no chip. */
export const isBlank = (d: Draft): boolean => d.typed === "" && d.pastes.length === 0;

/** The one string a draft goes as: each chip's text in paste order, as the chips are drawn above the field, then the
 * typed words, a blank line between. With
 * no chips, the typed words alone; with no typed words, the chips alone. Made only to send, or to check what was sent. */
export function sentOf(d: Draft): string {
  const tail = d.pastes.join(CHIP_GAP);
  if (tail === "") return d.typed;
  return d.typed === "" ? tail : tail + CHIP_GAP + d.typed;
}

/** `there` with `back` after it: the chips of both, `there`'s first, and the typed words of both, `sep` between. */
export function joinDrafts(there: Draft, back: Draft, sep: string): Draft {
  const typed =
    there.typed === ""
      ? back.typed
      : back.typed === ""
        ? there.typed
        : there.typed + sep + back.typed;
  return { typed, pastes: [...there.pastes, ...back.pastes] };
}

/** The questions sent lately, by the string they went as, newest last: what draftOf finds whole. Bounded, as each can
 * hold long texts. */
const SENT = new Map<string, Draft>();
const SENT_MAX = 16;

/** A question that went as `text`, held as the draft it was, so a string the host gives back is that draft again. */
export function rememberSent(text: string, d: Draft): void {
  if (d.pastes.length === 0) return;
  SENT.delete(text);
  SENT.set(text, d);
  if (SENT.size > SENT_MAX) SENT.delete(SENT.keys().next().value!);
}

/** Forgets every question remembered as sent. For tests only. */
export function forgetChipsForTests(): void {
  SENT.clear();
}

/** A string from outside the composer as a draft: a question this app sent as exactly `text`, as it was; else a text
 * of PASTE_CHIP_MIN units or more as one chip, whole, so the field never takes a long text; else typed words. Found
 * whole or not at all: never searched for chips inside it. */
export function draftOf(text: string): Draft {
  const sent = text.length >= PASTE_CHIP_MIN ? SENT.get(text) : undefined;
  if (sent !== undefined) return sent;
  return text.length >= PASTE_CHIP_MIN
    ? { typed: "", pastes: [text] }
    : { typed: text, pastes: [] };
}
