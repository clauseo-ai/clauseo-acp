// Where the composer's menu stands, and keeping it there: at the caret's x, 4 px clear of the field's drawn box,
// inside what clips it (the window, cut by the room's centre column it mounts in), as tall as the room it has; the lit
// row kept in the rows' view; placed again as the window resizes, anything scrolls under it or the field's own size
// changes while it is open. placeMenu is the rule apart from the page; caretOf and clipOf measure the page for it.
import { useEffect, useLayoutEffect, type RefObject } from "react";
import { FACE } from "./composer-menu-logic.ts";

/** The caret's line on the page: its x, and its line's top and bottom, in viewport px. */
export type Caret = { x: number; top: number; bottom: number };

/** A box on the page, in viewport px. */
export type Bounds = { left: number; top: number; right: number; bottom: number };

/** Where the menu stands: its left edge at the caret's x, and 4 px below the field, or 4 px above it when there is no
 * room below, as at the room's foot (`at` is the caret's x with the field's top and bottom, so it never covers the
 * field); kept 8 px inside the box that clips it (the window, or the room's column it mounts in), and as tall as the
 * room it has there. */
export function placeMenu(
  at: Caret,
  menu: { width: number; height: number },
  bounds: Bounds,
): { left: number; top: number; maxHeight: number } {
  const gap = 4;
  const edge = 8;
  const below = bounds.bottom - edge - (at.bottom + gap);
  const above = at.top - gap - (bounds.top + edge);
  const down = menu.height <= below || below >= above;
  const maxHeight = Math.max(0, Math.floor(down ? below : above));
  const height = Math.min(menu.height, maxHeight);
  const top = down ? at.bottom + gap : at.top - gap - height;
  const left = Math.max(bounds.left + edge, Math.min(at.x, bounds.right - edge - menu.width));
  return { left: Math.round(left), top: Math.round(top), maxHeight };
}

/** The caret's place, from a hidden copy of the field laid over it with a mark at the caret: the same face, size,
 * padding and wrapping, the field's client width (so a scrollbar can't move the wrapping) and its scroll. Built with
 * text nodes only. */
export function caretOf(field: HTMLTextAreaElement): Caret {
  const doc = field.ownerDocument;
  const style = getComputedStyle(field);
  const box = field.getBoundingClientRect();
  const copy = doc.createElement("div");
  // the field's box is read as drawn, its transform (the Devanagari nudge) in it, so the copy takes none
  for (const p of FACE) copy.style[p] = style[p];
  const borders = parseFloat(style.borderLeftWidth) + parseFloat(style.borderRightWidth);
  copy.style.position = "fixed";
  copy.style.left = `${box.left}px`;
  copy.style.top = `${box.top}px`;
  copy.style.width = `${field.clientWidth + (Number.isFinite(borders) ? borders : 0)}px`;
  copy.style.height = `${box.height}px`;
  copy.style.overflow = "hidden";
  copy.style.whiteSpace = "pre-wrap";
  copy.style.visibility = "hidden";
  copy.style.pointerEvents = "none";
  copy.setAttribute("aria-hidden", "true");
  const at = field.selectionEnd;
  const mark = doc.createElement("span");
  mark.textContent = "\u200b";
  copy.append(
    doc.createTextNode(field.value.slice(0, at)),
    mark,
    doc.createTextNode(field.value.slice(at) + "\u200b"),
  );
  doc.body.append(copy);
  copy.scrollTop = field.scrollTop;
  const r = mark.getBoundingClientRect();
  copy.remove();
  return { x: r.left, top: r.top, bottom: r.bottom };
}

/** The box a fixed plane mounted in `el` is seen in: the window, cut by each ancestor that clips its overflow (the
 * room's centre column clips, so a menu taller than the room above the field would lose its top rows behind it). The
 * root and body pass their overflow to the window, and a box drawn as its children only (display: contents) clips
 * nothing. */
export function clipOf(el: Element): Bounds {
  const doc = el.ownerDocument;
  const view = doc.defaultView;
  const b: Bounds = {
    left: 0,
    top: 0,
    right: view?.innerWidth ?? 0,
    bottom: view?.innerHeight ?? 0,
  };
  const clips = (v: string) => v !== "" && v !== "visible";
  for (
    let p = el.parentElement;
    p !== null && p !== doc.body && p !== doc.documentElement;
    p = p.parentElement
  ) {
    const s = getComputedStyle(p);
    if (s.display === "contents" || ![s.overflow, s.overflowX, s.overflowY].some(clips)) continue;
    const r = p.getBoundingClientRect();
    b.left = Math.max(b.left, r.left);
    b.top = Math.max(b.top, r.top);
    b.right = Math.min(b.right, r.right);
    b.bottom = Math.min(b.bottom, r.bottom);
  }
  return b;
}

/** Puts the open menu `el` at the caret's x, clear of the field's drawn box `frame` above or below it, inside what
 * clips it. Its height is read with its last bound let go: the rows up to their own cap, and the lines under them. */
function placeOpen(
  el: HTMLElement,
  field: HTMLTextAreaElement,
  frame: HTMLElement | null | undefined,
): void {
  const box = (frame ?? field).getBoundingClientRect();
  el.style.maxHeight = "";
  const at = placeMenu(
    { x: caretOf(field).x, top: box.top, bottom: box.bottom },
    { width: el.offsetWidth, height: el.offsetHeight },
    clipOf(el),
  );
  el.style.left = `${at.left}px`;
  el.style.top = `${at.top}px`;
  el.style.maxHeight = `${at.maxHeight}px`;
}

/** The lit row in the rows' view: the rows scroll by themselves, never the room's column under them, which
 * scrollIntoView would move too. */
function showLit(list: HTMLElement, hi: number): void {
  const lit = list.children.item(hi);
  if (lit === null) return;
  const r = lit.getBoundingClientRect();
  const b = list.getBoundingClientRect();
  if (r.top < b.top) list.scrollTop -= b.top - r.top;
  else if (r.bottom > b.bottom) list.scrollTop += Math.min(r.bottom - b.bottom, r.top - b.top);
}

/** Keeps the open menu placed. `plane` is the fixed box placed, `rows` the list inside it that scrolls, `hi` the
 * lit row's index, or null while the menu is closed. Its layout effect runs after each render, so call it where the
 * menu's own effects stand. */
export function useMenuPlace({
  field,
  frame,
  plane,
  rows,
  hi,
}: {
  field: HTMLTextAreaElement;
  /** The field's drawn box, which the menu stands clear of; the field itself without it. */
  frame: RefObject<HTMLElement | null> | undefined;
  plane: RefObject<HTMLDivElement | null>;
  rows: RefObject<HTMLDivElement | null>;
  hi: number | null;
}): void {
  // at the caret, once drawn, and the highlighted row in view
  useLayoutEffect(() => {
    const el = plane.current;
    if (el === null || hi === null) return;
    placeOpen(el, field, frame?.current);
    if (rows.current !== null) showLit(rows.current, hi);
  });

  // and again as the window resizes or anything scrolls under it, the field included; the rows' own scroll aside.
  // And as the field's own size changes: a narrower window rewraps the words and the field grows a line, but the
  // window's resize comes before that grow (the composer fits the field from its own size observer), so a menu placed
  // on the resize alone stood over the line's start (WebView2, M2 at 100 px narrower). Placed again before that frame paints.
  const isOpen = hi !== null;
  useEffect(() => {
    const view = field.ownerDocument.defaultView;
    if (!isOpen || view === null) return;
    const place = () => {
      if (plane.current !== null) placeOpen(plane.current, field, frame?.current);
    };
    const again = (e: Event) => {
      if (e.target !== plane.current && e.target !== rows.current) place();
    };
    view.addEventListener("resize", again);
    view.addEventListener("scroll", again, { capture: true, passive: true });
    // In the observer's own callback, which runs after layout and before paint: deferring to the next frame left one
    // painted frame with the grown field under the menu (WebView2, 3.3b hi3: -22 px for one frame, 4 runs of 4).
    const grown = typeof ResizeObserver === "function" ? new ResizeObserver(place) : null;
    grown?.observe(field);
    return () => {
      view.removeEventListener("resize", again);
      view.removeEventListener("scroll", again, { capture: true });
      grown?.disconnect();
    };
  }, [isOpen, field, frame, plane, rows]);
}
