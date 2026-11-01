// Marks where a string shows in a rendered state: the elements whose own text is the string, or whose accessible
// name or tooltip is, so the eye finds it at once in the frame.

export const MARK = "data-review-mark";

/** Marks the elements under root that show text; returns how many. */
export function markText(root: Element, text: string): number {
  const want = text.trim();
  let marked = 0;
  for (const el of root.querySelectorAll("*")) {
    const own = [...el.childNodes]
      .filter((n) => n.nodeType === Node.TEXT_NODE)
      .map((n) => n.textContent ?? "")
      .join("")
      .trim();
    const named = el.getAttribute("aria-label") === want || el.getAttribute("title") === want;
    if (own === want || named) {
      el.setAttribute(MARK, "");
      marked++;
    }
  }
  return marked;
}
