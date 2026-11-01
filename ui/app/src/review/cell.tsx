// One registered state, in the language and light the frame's query asks for, with its string marked.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { useEffect, useRef } from "react";
import { markText } from "./mark.ts";
import { REGISTRY } from "./registry.tsx";

export function Cell({ id, lang }: { id: string; lang: Lang }) {
  const entry = REGISTRY.find((e) => e.id === id);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (entry === undefined || root.current === null) return;
    const el = root.current;
    const text = entry.words.text(lang);
    // the parts may fill in after the first commit (fonts, popovers); mark again on the next frame
    markText(el, text);
    const frame = requestAnimationFrame(() => markText(el, text));
    return () => cancelAnimationFrame(frame);
  }, [entry, lang]);
  if (entry === undefined)
    return <p className="review-missing">No registered string has the id {id}.</p>;
  return (
    <div ref={root} className="review-cell">
      {entry.render(lang)}
    </div>
  );
}
