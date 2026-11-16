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

/** Presses each control in order (CSS selectors, a dialog's included, in its portal), each once it is on the page
 * (UD2's Plans: a confirm, then its act). */
export function PressInOrder({
  press,
  children,
}: {
  press: readonly string[];
  children: ReactNode;
}) {
  const done = useRef(0);
  useEffect(() => {
    const take = () => {
      const selector = press[done.current];
      if (selector === undefined) return;
      const el = document.querySelector<HTMLElement>(selector);
      if (el === null) return;
      done.current += 1;
      el.click();
    };
    const watch = new MutationObserver(take);
    watch.observe(document.body, { subtree: true, childList: true });
    take();
    return () => watch.disconnect();
  }, [press]);
  return children;
}
