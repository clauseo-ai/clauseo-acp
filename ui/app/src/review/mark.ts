// Marks where a string shows in a rendered state: the innermost elements whose text is the string (Say may split it
// into runs by script, so an element's whole text counts, not only its own text nodes), the elements whose own text
// holds it (a string that is one sentence of a longer line), or whose accessible name or tooltip is, so the eye finds
// it at once in the frame. A slot in the string ("Resets {time}", {path}) matches any words in its place.

export const MARK = "data-review-mark";

const escape = (words: string) => words.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** A whole-string pattern for a registered string: its words as written, each {slot} any words. */
export function patternOf(text: string): RegExp {
  return new RegExp(
    `^${text
      .trim()
      .split(/\{\w+\}/)
      .map(escape)
      .join(".+?")}$`,
    "s",
  );
}

/** Marks the elements under root that show text; returns how many. */
export function markText(root: Element, text: string): number {
  const want = patternOf(text);
  // its slots stand for any words; elsewhere the string as written, anywhere in an element's own text as whole words
  // ("Needs you" is not held by "Needs your permission")
  const held = new RegExp(
    `(?<![\\p{L}\\p{M}\\p{N}])${want.source.slice(1, -1)}(?![\\p{L}\\p{M}\\p{N}])`,
    "su",
  );
  const is = (value: string | null) => value !== null && want.test(value.trim());
  const shows = (el: Element) => is(el.textContent);
  let marked = 0;
  for (const el of root.querySelectorAll("*")) {
    const innermost = shows(el) && ![...el.children].some(shows);
    const own = [...el.childNodes]
      .filter((n) => n.nodeType === Node.TEXT_NODE)
      .map((n) => n.textContent ?? "")
      .join("")
      .trim();
    const holds = own !== "" && held.test(own);
    if (innermost || holds || is(el.getAttribute("aria-label")) || is(el.getAttribute("title"))) {
      el.setAttribute(MARK, "");
      marked++;
    }
  }
  return marked;
}
