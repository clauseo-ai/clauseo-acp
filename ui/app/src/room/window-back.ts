// The window coming back to the lawyer: its own `focus` event (an element's focus doesn't reach it), or the document
// shown again. Apart from conversations.ts, which hears it for H6's marks, to keep that file under the size limit.

/** Calls `back` each time the window comes back; returns the stop. Nothing outside a page. */
export function onWindowBack(back: () => void): () => void {
  if (typeof window === "undefined") return () => {};
  const focused = (e: FocusEvent) => {
    if (!(e.target instanceof Node)) back();
  };
  const shown = () => {
    if (document.visibilityState === "visible") back();
  };
  window.addEventListener("focus", focused);
  document.addEventListener("visibilitychange", shown);
  return () => {
    window.removeEventListener("focus", focused);
    document.removeEventListener("visibilitychange", shown);
  };
}
