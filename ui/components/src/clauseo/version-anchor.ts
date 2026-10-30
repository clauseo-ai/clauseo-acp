// The earlier draft lies open at the same paragraph. A hook, kept apart from the parts.
import { useLayoutEffect, useRef, type RefObject } from "react";

/** How far below the desk's top a paragraph opened to stands: the room's 32 px above a thread. */
const OPEN_AT = 32;

/** Keeps the same paragraph where it stood in view when the sheet changes under it: before the change, the first
 * paragraph in view and its distance from the desk's top; after it, the same paragraph, or the one before it, back
 * at that distance. A sheet opened fresh lies open at its first paragraph that differs, or its first paragraph. And
 * when every paragraph that differs stands out of view, the first of them comes into view. Read on scroll, so
 * nothing runs at rest. */
export function useSameParagraph(key: string): RefObject<HTMLDivElement | null> {
  const scroller = useRef<HTMLDivElement>(null);
  const anchor = useRef<{ place: string; offset: number } | null>(null);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el === null) return;
    const read = () => {
      const top = el.getBoundingClientRect().top;
      for (const p of el.querySelectorAll<HTMLElement>(".ver-place")) {
        const r = p.getBoundingClientRect();
        if (r.bottom > top + 8) {
          anchor.current = { place: p.dataset["place"] ?? "", offset: r.top - top };
          return;
        }
      }
      anchor.current = null;
    };
    el.addEventListener("scroll", read, { passive: true });
    return () => el.removeEventListener("scroll", read);
  }, []);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el === null) return;
    const box = el.getBoundingClientRect();
    const all = [...el.querySelectorAll<HTMLElement>(".ver-place")];
    const marked = all.filter(
      (p) => p.dataset["change"] !== undefined || p.dataset["differs"] !== undefined,
    );
    const bring = (p: HTMLElement | undefined, offset: number) => {
      if (p === undefined) return;
      el.scrollTop = el.scrollTop + p.getBoundingClientRect().top - box.top - offset;
    };
    const a = anchor.current;
    if (a === null || el.scrollTop === 0) {
      bring(marked[0] ?? all[0], OPEN_AT);
      return;
    }
    bring(
      all.find((p) => p.dataset["place"] === a.place) ??
        all.filter((p) => (p.dataset["place"] ?? "") < a.place).pop(),
      a.offset,
    );
    const seen = (p: HTMLElement) => {
      const r = p.getBoundingClientRect();
      return r.bottom > box.top && r.top < box.bottom;
    };
    if (marked.length > 0 && !marked.some(seen)) bring(marked[0], OPEN_AT);
  }, [key]);
  return scroller;
}
