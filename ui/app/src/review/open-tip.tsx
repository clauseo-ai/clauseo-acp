// A review cell's wrapper that opens an info tip (ui/info-tip.tsx), so the cell shows the words behind the icon.
import { useEffect, useRef, type ReactNode } from "react";

/** Presses the info tip named `label` once its button is on the page (the settings load after the first paint), so
 * the cell shows what the tip holds. */
export function OpenTip({ label, children }: { label: string; children: ReactNode }) {
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = box.current;
    if (el === null) return;
    const press = () => {
      const button = el.querySelector<HTMLElement>(`button[aria-label="${label}"]`);
      if (button === null) return false;
      button.click();
      return true;
    };
    if (press()) return;
    const watch = new MutationObserver(() => {
      if (press()) watch.disconnect();
    });
    watch.observe(el, { childList: true, subtree: true });
    return () => watch.disconnect();
  }, [label]);
  return <div ref={box}>{children}</div>;
}
