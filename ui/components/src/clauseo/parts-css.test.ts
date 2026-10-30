import { readdirSync, readFileSync } from "node:fs";
import { expect, test } from "vitest";

const here = new URL("./", import.meta.url);

test("parts.css imports every stylesheet of Clauseo's parts, once", () => {
  const sheets = readdirSync(here).filter((f) => f.endsWith(".css") && f !== "parts.css");
  const imported = [
    ...readFileSync(new URL("parts.css", here), "utf8").matchAll(/@import "\.\/([\w-]+\.css)";/g),
  ].map((m) => m[1]);
  expect(sheets.length).toBeGreaterThan(10);
  expect([...imported].sort((a, b) => (a ?? "").localeCompare(b ?? ""))).toEqual(
    sheets.sort((a, b) => a.localeCompare(b)),
  );
});

test("on a lit tile, every line of its face takes the tile's ink, never the quiet tone", () => {
  const wall = readFileSync(new URL("wall.css", here), "utf8");
  const inked = (part: string) =>
    new RegExp(
      `\\.tile:not\\(\\[data-state="uncited"\\]\\) \\.${part} \\{[^}]*color: currentColor;`,
    ).test(wall);
  for (const part of ["tile-num", "tile-title", "tile-to"]) expect(inked(part), part).toBe(true);
  expect(inked("tile-nothing")).toBe(false);
});

test("on a phone's width a list of dates and the room's band stack each label over its words, and the reader's bar wraps", () => {
  const page = readFileSync(new URL("version-page.css", here), "utf8");
  const narrow = page.slice(page.indexOf("@variant max-narrow"));
  expect(narrow).toMatch(/\.ver-date-row \{[^}]*grid-template-columns: minmax\(0, 1fr\);/);
  expect(page.indexOf("@variant max-narrow")).toBeGreaterThan(page.indexOf(".ver-date-row {"));
  const limits = readFileSync(new URL("limits.css", here), "utf8");
  expect(limits.slice(limits.indexOf("@variant max-narrow"))).toMatch(
    /\.lim-band-rows \{[^}]*grid-template-columns: minmax\(0, 1fr\);/,
  );
  const reader = readFileSync(new URL("reader.css", here), "utf8");
  expect(reader).toMatch(/\.rd-what \{[^}]*flex-wrap: wrap;/);
  expect(reader).not.toMatch(/\.rd-what \{[^}]*flex-wrap: nowrap;/);
  const phone = reader.slice(reader.indexOf("@variant max-narrow"));
  expect(phone).toMatch(/\.rd-bar \{[^}]*flex-wrap: wrap;/);
  expect(phone).toMatch(/\.rd-what \{[^}]*flex-basis: 100%;/);
});
