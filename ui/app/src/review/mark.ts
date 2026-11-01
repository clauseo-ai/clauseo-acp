// Marks where a string shows in a rendered state: the elements whose own text is the string or holds it (a slot such
// as {path} standing for any words, and a string that is one sentence of a longer line), or whose accessible name or
// tooltip is, so the eye finds it at once in the frame.

export const MARK = "data-review-mark";

/** Marks the elements under root that show text; returns how many. */
export function markText(root: Element, text: string): number {
  const want = text.trim();
  // its slots stand for any words; elsewhere the string as written
  const held = new RegExp(
    want
      .split(/\{\w+\}/)
      .map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
      .join(".+?"),
    "u",
  );
  let marked = 0;
  for (const el of root.querySelectorAll("*")) {
    const own = [...el.childNodes]
      .filter((n) => n.nodeType === Node.TEXT_NODE)
      .map((n) => n.textContent ?? "")
      .join("")
      .trim();
    const named = el.getAttribute("aria-label") === want || el.getAttribute("title") === want;
    if (own === want || (own !== "" && held.test(own)) || named) {
      el.setAttribute(MARK, "");
      marked++;
    }
  }
  return marked;
}
