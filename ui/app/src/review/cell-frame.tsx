// A frame showing one registered state in one language and light, rendered in a window of the entry's size (a desktop
// one by default, so the room's breakpoints follow a window and not the cell) and scaled down to fit the cell.
import type { Lang, Platform, Theme } from "@clauseo/components/lib/display.ts";
import { useEffect, useRef, useState } from "react";

type Props = {
  id: string;
  lang: Lang;
  theme: Theme;
  platform: Platform;
  /** The window's size in CSS px. */
  width: number;
  height: number;
};

export function CellFrame({ id, lang, theme, platform, width, height }: Props) {
  const box = useRef<HTMLDivElement>(null);
  const [scale, setScale] = useState(1);
  useEffect(() => {
    const el = box.current;
    if (el === null) return;
    const fit = () => setScale(Math.min(1, el.clientWidth / width));
    fit();
    const watch = new ResizeObserver(fit);
    watch.observe(el);
    return () => watch.disconnect();
  }, [width]);
  const src = `?${new URLSearchParams({ cell: id, lang, theme, platform }).toString()}`;
  const language = lang === "en" ? "English" : "Hindi";
  return (
    <figure className="review-figure">
      <figcaption>
        {language} · {theme}
        {scale < 1 && <span className="review-scale"> · {Math.round(scale * 100)}%</span>}
      </figcaption>
      {/* styles set as properties, which the CSP's style-src 'self' allows, unlike a style attribute in markup */}
      <div ref={box} className="review-box" style={{ height: Math.ceil(height * scale) }}>
        <iframe
          key={src}
          src={src}
          title={`${id}, ${language}, ${theme}, ${platform}`}
          loading="lazy"
          width={width}
          height={height}
          className="review-frame"
          style={{ transform: `scale(${scale})` }}
        />
      </div>
    </figure>
  );
}
