// A review cell's wrapper that presses a button in the window's pane by its words (U15: Add credits, Use an API key
// instead, Open the payment page), so the cell shows what the press opens, as OpenTip does for an info tip.
import { useEffect, useRef, type ReactNode } from "react";

/** Presses the button whose words are `words` once it is on the page (the section loads after the first paint). */
export function PressWords({ words, children }: { words: string; children: ReactNode }) {
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = box.current;
    if (el === null) return;
    const press = () => {
      const button = [...el.querySelectorAll<HTMLButtonElement>("main button")].find(
        (b) => b.textContent === words,
      );
      if (button === undefined) return false;
      button.click();
      return true;
    };
    if (press()) return;
    const watch = new MutationObserver(() => {
      if (press()) watch.disconnect();
    });
    watch.observe(el, { childList: true, subtree: true });
    return () => watch.disconnect();
  }, [words]);
  return <div ref={box}>{children}</div>;
}
