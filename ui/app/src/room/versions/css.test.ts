// Versions' sheets, read as text: the where popover's title stands flush with the lines under it (no inset of the
// allows card's), and a line for what just happened, as the card's first row at now, starts as far below the card's
// edge as the band's row does.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

const design = readFileSync(
  new URL("../../../../components/src/clauseo/versions.css", import.meta.url),
  "utf8",
);
const room = readFileSync(new URL("versions.css", import.meta.url), "utf8");

/** The body of the first rule for a selector. */
const rule = (css: string, selector: string): string | undefined =>
  css.split(`${selector} {`)[1]?.split("}")[0];

test("the where popover's title has no inset of its own", () => {
  const title = rule(design, ".ver-where-title");
  expect(title).toBeDefined();
  expect(title).not.toMatch(/padding/);
});

test("the positioner's note stands right above the rule it describes", () => {
  expect(design).toMatch(/takes the floating layer's place \*\/\n {2}\.ver-where \{/);
});

test("a line under the band, as the card's first row, starts at the band's top padding", () => {
  expect(rule(room, ".ver-room-band > .ver-notice:first-child")).toMatch(
    /padding-top: var\(--s3\);/,
  );
});
