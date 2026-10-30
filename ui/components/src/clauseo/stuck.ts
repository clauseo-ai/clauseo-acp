// A heading that holds the top of its list while the rest scrolls under it casts --shadow-pin once
// it is stuck, as the pinned question does, so what passes under it reads as passing under an edge, never as words cut
// in half. Read on scroll, and once when the list changes, so nothing runs at rest.
import { useLayoutEffect, type RefObject } from "react";

/** Marks data-stuck on each heading the selector finds inside the scroller while it holds the top: its section has
 * scrolled up past the scroller's top and the heading stands at it. */
export function useStuck(scroller: RefObject<HTMLElement | null>, selector: string, key: unknown) {
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el === null) return;
    const mark = () => {
      const top = el.getBoundingClientRect().top;
      for (const h of el.querySelectorAll<HTMLElement>(selector)) {
        const box = h.parentElement === el ? null : h.parentElement;
        const r = h.getBoundingClientRect();
        // a heading straight in the scroller is stuck as soon as the scroller has scrolled; one in a section, while its
        // section runs past the top under it
        const stuck =
          box === null
            ? el.scrollTop > 0
            : r.top - top <= Math.max(0, Number.parseFloat(getComputedStyle(h).top) || 0) + 1 &&
              box.getBoundingClientRect().top < r.top - 1;
        if (stuck) h.dataset["stuck"] = "";
        else delete h.dataset["stuck"];
      }
    };
    mark();
    el.addEventListener("scroll", mark, { passive: true });
    return () => el.removeEventListener("scroll", mark);
  }, [scroller, selector, key]);
}
