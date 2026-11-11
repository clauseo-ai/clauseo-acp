// The lawyer's question in the thread, folded after eight lines with "Show all", and "Show less" once open. The fold
// is CSS's line clamp (thread.css, .asked[data-folded]): no render measures the text. A question past FOLD_CHARS
// characters is past eight lines at every width and text size the room has, so it folds without a measure and draws
// only its start while folded: a 50-page or 6 MiB question costs a few lines of layout until Show all. A shorter one
// is drawn whole under the clamp and measured only as its box changes size (a ResizeObserver), to know whether the
// clamp cut anything; eight lines or fewer, it looks as it always did and offers no toggle.
import { useId, useLayoutEffect, useRef, useState, type Ref } from "react";
import { langOfText } from "../lib/script.ts";
import { th } from "../lib/words-thread.ts";
import { useWords } from "../lib/words.ts";
import { FOLD_CHARS, startOf } from "./question-fold-logic.ts";
import { Say } from "./say.tsx";

/** The question's words, folded past eight lines. `ref` is the paragraph's (the room's "asked" node, which the
 * pinned question measures). */
export function FoldedQuestion({ text, ref }: { text: string; ref?: Ref<HTMLParagraphElement> }) {
  const w = useWords();
  const id = useId();
  const long = text.length > FOLD_CHARS;
  const [open, setOpen] = useState(false);
  // whether the clamp cuts a short question: known only by measure, once its box has a size
  const [cut, setCut] = useState(false);
  const own = useRef<HTMLParagraphElement | null>(null);
  const toggle = useRef<HTMLButtonElement | null>(null);
  // Show less asked for: once folded again, the question is brought back into view
  const closing = useRef(false);

  useLayoutEffect(() => {
    const p = own.current;
    if (long || open || p === null) return;
    const measure = () => setCut(p.scrollHeight > p.clientHeight + 1);
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(p);
    return () => ro.disconnect();
  }, [long, open, text]);

  useLayoutEffect(() => {
    if (open || !closing.current) return;
    closing.current = false;
    // the question's row stands clear of the pinned bar by its scroll margin (shell.css)
    const row = own.current?.closest("[data-entry-id]") ?? own.current;
    const top = row?.getBoundingClientRect().top ?? 0;
    // what covers the view's top: the pinned bar, or the edge of the thread where the thread scrolls on its own
    const pin = document.querySelector<HTMLElement>(".room-pin:not([hidden])");
    const thread = row?.closest<HTMLElement>(".room-thread");
    const scrolls = thread != null && getComputedStyle(thread).overflowY !== "visible";
    const clear = Math.max(
      pin === null ? 0 : pin.getBoundingClientRect().bottom,
      scrolls ? thread.getBoundingClientRect().top : 0,
    );
    if (top < clear) row?.scrollIntoView({ block: "start" });
  }, [open]);

  const folds = long || cut || open;
  const shown = long && !open ? startOf(text, FOLD_CHARS) : text;
  return (
    <>
      <p
        id={id}
        className="asked"
        ref={(node) => {
          own.current = node;
          if (typeof ref === "function") return ref(node);
          if (ref != null) ref.current = node;
        }}
        lang={langOfText(text)}
        data-folded={open ? undefined : ""}
      >
        <Say>{shown}</Say>
      </p>
      {folds && (
        <button
          type="button"
          ref={toggle}
          className="asked-fold"
          aria-expanded={open}
          aria-controls={id}
          // the thread lets go of its end on this press (thread.tsx), so the question stays where the lawyer reads
          data-fold=""
          onClick={() => {
            closing.current = open;
            setOpen(!open);
          }}
        >
          <Say>{th(w.lang, open ? "Show less" : "Show all")}</Say>
        </button>
      )}
    </>
  );
}
