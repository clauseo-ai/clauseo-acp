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
    "IconPageTurn",
    "IconPaperBookPage",
    "IconSafeWorkspace",
  ]);
  const lucide = new Set(Object.keys(icons).filter((name) => !(name in glyphs)));
  expect(lucide.size).toBe(ICON_NAMES.length - 4);
});
