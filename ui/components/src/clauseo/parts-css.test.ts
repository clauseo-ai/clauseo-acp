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

test("Linux takes Windows' chrome: the toolbar's rules for the caption buttons key on any platform but the Mac", () => {
  const sheets = readdirSync(here).filter((f) => f.endsWith(".css"));
  const keyed = sheets.flatMap((f) =>
    [...readFileSync(new URL(f, here), "utf8").matchAll(/\[data-platform="(\w+)"\]/g)].map(
      (m) => `${f}: ${m[0]}`,
    ),
  );
  expect(keyed.length).toBeGreaterThan(0);
  for (const k of keyed) expect(k).toMatch(/\[data-platform="mac"\]$/);
  const shell = readFileSync(new URL("shell.css", here), "utf8");
  expect(shell).toMatch(
    /\.room-top:not\(\[data-platform="mac"\]\) \.room-lead \{[^}]*display: none;/,
  );
});

test("a facade of files known by their names keeps no room for a numeral or a chip: its rows are title and padding", () => {
  const wall = readFileSync(new URL("wall.css", here), "utf8");
  const tileH = /\.facade \{[^}]*--tile-h: calc\(([^;]*)\);/.exec(wall)?.[1] ?? "";
  // the full facade's row keeps both
  expect(tileH).toContain("var(--num-h)");
  expect(tileH).toContain("var(--slot-h)");
  expect(wall).toMatch(
    /\.facade-names \{\s*--tile-h: calc\(var\(--tile-pad\) \+ var\(--title-h\) \+ var\(--tile-pad\)\);\s*\}/,
  );
  expect(wall).toMatch(/\.facade-names \.tile-face \{\s*padding-bottom: var\(--tile-pad\);\s*\}/);
});

test("the folded rail opens from the window's top, just under Matters, and an empty one draws no panel", () => {
  const responsive = readFileSync(new URL("responsive.css", here), "utf8");
  const folded = responsive.slice(
    responsive.indexOf("@variant rail-folded"),
    responsive.indexOf("@variant max-mid"),
  );
  expect(folded).toMatch(/\.room-rail \{[^}]*top: var\(--rail-panel-top\);/);
  // an absolutely placed grid item counts its top from its grid area unless it has none
  expect(folded).toMatch(/\.room-rail \{\s*grid-area: auto;\s*\}/);
  const empty = /\.room-rail:empty \{([^}]*)\}/.exec(folded)?.[1] ?? "";
  expect(empty).toContain("background: transparent;");
  expect(empty).toContain("box-shadow: none;");
  expect(empty).toContain("pointer-events: none;");
  expect(folded.indexOf(".room-rail:empty")).toBeGreaterThan(
    folded.indexOf("box-shadow: var(--e3)"),
  );
});

test("a tile's title, a long Devanagari name too, wraps anywhere a long name needs and is never clamped or clipped", () => {
  // Devanagari words run long (a compound with no space, a name filed without spaces): the title wraps, as the
  // engine's line breaker allows, which keeps a syllable whole; the row grows with it (.facade's grid-auto-rows). No
  // clamp, ellipsis or hidden overflow cuts a name, so no syllable is ever half drawn. The real engines' line breaks
  // are measured on the probe's ?papers=hindi wall (scripts/webkit-probe.swift).
  const wall = readFileSync(new URL("wall.css", here), "utf8");
  const rules = [...wall.matchAll(/([^{}]*\.tile-title[^{}]*)\{([^}]*)\}/g)];
  expect(rules.length).toBeGreaterThan(0);
  const base = rules.find((r) => r[1]?.trim() === ".tile-title")?.[2] ?? "";
  expect(base).toMatch(/overflow-wrap: anywhere;/);
  expect(base).toMatch(/display: block;/);
  for (const [, selector, body] of rules) {
    expect(body, selector).not.toMatch(
      /line-clamp|text-overflow|overflow: hidden|white-space: nowrap|max-height/,
    );
  }
  expect(wall).toMatch(/\.facade \{[^}]*grid-auto-rows: minmax\(var\(--tile-h\), auto\);/);
});
