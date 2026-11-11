// D-074: from 1600 px (full screen among them) the wall takes more of the width, with as many columns of tiles as it
// holds, and the reading column keeps its measure. happy-dom lays nothing out, so the rules are read from the
// stylesheets and the widths worked out from the tokens; the 1920 by 1080 screenshots are in the PR.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

const read = (file: string) => readFileSync(new URL(file, import.meta.url), "utf8");
const css = read("responsive.css");
const parts = read("../styles/tokens-parts.css");
const theme = read("../styles/tokens-theme.css");

/** A token's value in tokens-parts.css, which the formatter may wrap. */
const token = (name: string) => new RegExp(`${name}: ([^;]+);`).exec(parts)?.[1] ?? "";
const px = (name: string) => Number(/^(\d+)px$/.exec(token(name))?.[1]);

/** The `@variant room-wide` block. */
function wide(): string {
  const at = css.indexOf("@variant room-wide {");
  expect(at).toBeGreaterThan(-1);
  return css.slice(at, css.indexOf("@variant", at + 1));
}

/** clamp(min, vw, max) in px at a window `w` px wide. */
function clamp(value: string, w: number): number {
  const m = /^clamp\(\s*(\d+)px,\s*(\d+)vw,\s*(\d+)px\s*\)$/.exec(value.trim());
  expect(m).not.toBeNull();
  const [min, vw, max] = [Number(m![1]), Number(m![2]), Number(m![3])];
  return Math.min(max, Math.max(min, (vw / 100) * w));
}

test("the wide room starts at 1600 px, above the room's three-column width", () => {
  expect(theme).toContain("@custom-variant room-wide (@media (min-width: 1600px));");
});

test("there the wall takes the wide width, and its tiles fill as many columns as fit", () => {
  const block = wide();
  expect(block).toMatch(/--pane-w: var\(--pane-w-wide\);/);
  expect(block).toMatch(
    /\.facade \{\s*grid-template-columns: repeat\(auto-fill, minmax\(var\(--tile-min-w\), 1fr\)\);/,
  );
});

test("the wall gains columns at 1600 and 1920, and the reading column keeps its measure beside it", () => {
  const rail = (w: number) => clamp(token("--rail-w"), w);
  for (const [w, least] of [
    [1600, 4],
    [1920, 5],
  ] as const) {
    const pane = clamp(token("--pane-w-wide"), w);
    // the wall's own sides take what the 416 px wall gave its 351 px facade at 1920 before (measured)
    const facade = pane - 65;
    const columns = Math.floor(
      (facade + px("--tile-gap")) / (px("--tile-min-w") + px("--tile-gap")),
    );
    expect(columns, `${w} px`).toBeGreaterThanOrEqual(least);
    // what's left for the work holds the measure and both its sides' padding
    const work = w - rail(w) - pane + px("--overlap");
    expect(work, `${w} px`).toBeGreaterThanOrEqual(px("--measure") + 2 * px("--thread-pad"));
  }
});
