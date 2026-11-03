// For tests only: an icon in place. The element holds that icon's own drawing, at that size, hidden from assistive
// technology; rendering the icon alone gives its drawing to look for, so one shape is told from another.
import type { ComponentType } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import type { IconProps } from "./icon.tsx";

const SHAPE_ATTRS = ["d", "cx", "cy", "r", "fill"] as const;

/** An svg's drawing as one string: its size and class, and each shape's tag and geometry. */
function drawing(svg: Element): string {
  const shapes = [...svg.children].map(
    (c) =>
      `${c.tagName.toLowerCase()}:${SHAPE_ATTRS.map((a) => c.getAttribute(a) ?? "").join(",")}`,
  );
  return [svg.getAttribute("width"), svg.getAttribute("class"), ...shapes].join("|");
}

/** The same drawing, read from the icon's rendered markup. */
function drawingOf(markup: string): string {
  const attr = (tag: string, name: string) =>
    new RegExp(`\\s${name}="([^"]*)"`).exec(tag)?.[1] ?? "";
  const svg = /<svg[^>]*>/.exec(markup)?.[0] ?? "";
  const shapes = [...markup.matchAll(/<(path|circle)\b[^>]*>/g)].map(
    ([tag, name]) => `${name}:${SHAPE_ATTRS.map((a) => attr(tag, a)).join(",")}`,
  );
  return [attr(svg, "width"), attr(svg, "class"), ...shapes].join("|");
}

/** The svg in `el` that draws `Icon` at `size` (with `className`, where the part gives it one), or null. */
export function iconIn(
  el: Element | null | undefined,
  Icon: ComponentType<IconProps>,
  size: NonNullable<IconProps["size"]>,
  className?: string,
): SVGSVGElement | null {
  if (el === null || el === undefined) return null;
  const want = drawingOf(
    renderToStaticMarkup(
      className === undefined ? <Icon size={size} /> : <Icon size={size} className={className} />,
    ),
  );
  return [...el.querySelectorAll("svg")].find((s) => drawing(s) === want) ?? null;
}
