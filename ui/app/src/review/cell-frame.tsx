// A frame showing one registered state in one language and light. Its height follows the state's: the frame is the
// page's own origin, so the list reads its document's height.
import type { Lang, Theme } from "@clauseo/components/lib/display.ts";
import { useEffect, useRef, useState } from "react";

export function CellFrame({ id, lang, theme }: { id: string; lang: Lang; theme: Theme }) {
  const ref = useRef<HTMLIFrameElement>(null);
  const [height, setHeight] = useState(120);
  const [loaded, setLoaded] = useState(0);
  useEffect(() => {
    const doc = ref.current?.contentDocument;
    const root = doc?.getElementById("root");
    if (loaded === 0 || root === null || root === undefined) return;
    // #root holds just the state, so its height is the state's, where the document's would never shrink below the
    // frame's own
    const fit = () => setHeight(Math.max(48, Math.ceil(root.getBoundingClientRect().height)));
    fit();
    const watch = new ResizeObserver(fit);
    watch.observe(root);
    return () => watch.disconnect();
  }, [loaded]);
  // the same page, with the platform pinned so every reviewer sees the same chrome
  const src = `?${new URLSearchParams({ cell: id, lang, theme, platform: "mac" }).toString()}`;
  return (
    <figure className="review-figure">
      <figcaption>
        {lang === "en" ? "English" : "Hindi"} · {theme}
      </figcaption>
      <iframe
        ref={ref}
        src={src}
        title={`${id}, ${lang === "en" ? "English" : "Hindi"}, ${theme}`}
        loading="lazy"
        height={height}
        className="review-frame"
        onLoad={() => setLoaded((n) => n + 1)}
      />
    </figure>
  );
}
