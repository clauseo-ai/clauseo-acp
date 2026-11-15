// The composer's field fitted to its words: one line at the stylesheet's height, growing to six, then scrolling.
import { useLayoutEffect, type RefObject } from "react";
import type { Lang, TextSize } from "../lib/display.ts";
import { insertedOnly } from "./composer-edit.ts";

/** How long typing pauses before a field only grown as words went in is fitted whole (ms). */
const SETTLE_MS = 400;

/** Fits the field `area` to its words as the display's language or text size changes and as its width does, and sets
 * `fitRef` to the fit the composer runs after each change: `grow` after an input event, when words only put in can
 * only keep or grow it, `kind` the event's inputType. `fitted` holds the words as the field was last fitted to them. */
export function useFieldFit(
  area: RefObject<HTMLTextAreaElement | null>,
  fitted: RefObject<string>,
  fitRef: RefObject<(grow?: boolean, kind?: string) => void>,
  lang: Lang,
  textSize: TextSize,
) {
  useLayoutEffect(() => {
    const a = area.current;
    if (a === null) return;
    // the one line's height, as the last whole fit measured it
    let one = 0;
    // each style written only when it changes: a write of the same value still restyles the field in some engines
    const set = (height: string, scrolls: boolean) => {
      if (a.style.height !== height) a.style.height = height;
      const overflow = scrolls ? "" : "hidden";
      if (a.style.overflowY !== overflow) a.style.overflowY = overflow;
      if (!scrolls && a.scrollTop !== 0) a.scrollTop = 0;
    };
    // a whole fit once typing pauses, after fits that only grew the field
    let settle: ReturnType<typeof setTimeout> | undefined;
    const fit = (grow = false, kind = "") => {
      clearTimeout(settle);
      settle = undefined;
      const was = fitted.current;
      // macOS commits a dead key's mark by taking it out and putting it back, in one task: no fit between (#117 L)
      const out = kind === "deleteCompositionText";
      if (!out) fitted.current = a.value;
      // over the cap, a scroll height past the field's height is its words' own: the field stays, with no diff, no
      // collapse and no settle. After a 6 MiB paste the diff took 11 ms a key; Backspace's whole fit 149 ms to 108
      if (one > 0 && a.style.height === `${one * 6}px` && a.style.overflowY === "") {
        if (a.scrollHeight > a.clientHeight) return;
      }
      // words only put in, as plain typing does: the field can only keep its height or grow, which its own scroll
      // height shows with no collapse. Collapsing it to measure, as the whole fit does, laid out the room's column
      // twice and repainted the thread above on every keystroke (Chromium trace, #117 L). A mark that joins a
      // conjunct can narrow a line enough to lose one, which a scroll height never shows, and a mark can go for good:
      // so once typing pauses, a field taller than one line is fitted whole
      if (grow && one > 0 && (out || insertedOnly(was, a.value))) {
        const h = a.scrollHeight;
        if (h > a.clientHeight + (a.style.height === "" ? 4 : 0))
          set(`${Math.min(h, one * 6)}px`, h > one * 6);
        if (a.style.height !== "") settle = setTimeout(() => fit(), SETTLE_MS);
        return;
      }
      a.style.height = "";
      one = a.clientHeight;
      const cap = one * 6;
      const h = a.scrollHeight;
      // only words past the six lines scroll; short of them the field draws no scroll arrows. A Devanagari line can
      // measure a few px over the one line, inside the slack above, and WebView2 drew arrows on an empty field
      set(h > one + 4 ? `${Math.min(h, cap)}px` : "", h > cap);
    };
    fitRef.current = fit;
    fit();
    // the fit can change the field's height and so the form's, which the status line's observer, watching the form,
    // has already been told of this frame: the browser keeps that change for the next frame and reports
    // "ResizeObserver loop completed with undelivered notifications" once. It is no loop: the next delivery finds
    // the width unchanged and fits nothing. (Chromium, a composer at 390 px whose width alone changes.)
    // The WebKit probe must allow this one message ("ResizeObserver loop completed with undelivered notifications"),
    // decided in review: it fires once per width change and settles.
    let width = a.clientWidth;
    const ro = new ResizeObserver(() => {
      if (a.clientWidth === width) return;
      width = a.clientWidth;
      fit();
    });
    ro.observe(a);
    return () => {
      ro.disconnect();
      clearTimeout(settle);
    };
  }, [area, fitted, fitRef, lang, textSize]);
}
