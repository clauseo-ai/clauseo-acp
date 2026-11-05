import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { expect, test } from "vitest";
import * as glyphs from "./glyphs.tsx";
import * as icons from "./icons.tsx";
import { ICON_NAMES } from "./names.ts";

const sorted = (names: readonly string[]) => [...names].sort((a, b) => a.localeCompare(b));

test("names.ts lists every icon icons.tsx exports, once", () => {
  expect(sorted(ICON_NAMES.map((n) => n.component))).toEqual(sorted(Object.keys(icons)));
});

test("Clauseo's own glyphs live in glyphs.tsx, outside the Lucide file's licence", () => {
  expect(sorted(Object.keys(glyphs))).toEqual([
    "IconAccept",
    "IconFileEmail",
    "IconFileGeneric",
    "IconFileMarkdown",
    "IconFilePdf",
    "IconFileWord",
    "IconPageTurn",
    "IconPaperBookPage",
    "IconSafeWorkspace",
  ]);
  const lucide = new Set(Object.keys(icons).filter((name) => !(name in glyphs)));
  expect(lucide.size).toBe(ICON_NAMES.length - 9);
});

/** Each vertical line's x in an icon's paths: the x of every "M{x} {y}v…" or "…V…" segment that starts one. */
const verticals = (markup: string) =>
  [...markup.matchAll(/d="M([\d.]+)[ ,][\d.]+[vV]/g)].map((m) => Number(m[1]));

test("the browser's ticks and the download's shaft and tray sides land on the pixel grid at 1x", () => {
  // a 1.5 px stroke centred at x.75 covers whole pixels from x, as the outlines' 2.75 do
  for (const icon of [icons.IconBrowser, icons.IconDownload]) {
    const xs = verticals(renderToStaticMarkup(createElement(icon, { size: 20 })));
    expect(xs.length).toBeGreaterThan(0);
    for (const x of xs) expect(x % 1, `${icon.name} at ${x}`).toBe(0.75);
  }
});
