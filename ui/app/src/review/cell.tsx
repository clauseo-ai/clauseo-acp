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
    // the whole frame's body, not the cell alone: a dialog stands in a portal outside the cell
    const el = root.current.ownerDocument.body;
    const text = entry.words.text(lang);
    // the state fills in as its scripted host answers; mark again on each change of content (marks are attributes,
    // which this doesn't watch, so marking never wakes it)
    markText(el, text);
    const watch = new MutationObserver(() => markText(el, text));
    watch.observe(el, { childList: true, subtree: true, characterData: true });
    return () => watch.disconnect();
  }, [entry, lang]);
  if (entry === undefined)
    return <p className="review-missing">No registered string has the id {id}.</p>;
  return (
    <div ref={root} className="review-cell">
      {entry.render(lang)}
    </div>
  );
}
