import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { blocks } from "./css-blocks.ts";

const css = readFileSync(new URL("./tokens.css", import.meta.url), "utf8");
const list = blocks(css);
/** The day values: every custom property set on :root outside any at-rule. */
const day = new Map(
  list.filter((b) => b.selector === ":root" && b.context === "").flatMap((b) => [...b.decls]),
);

test("a Latin run inside Hindi keeps its tracking", () => {
  // Hindi zeroes the tracking of Devanagari runs, which would break the headline joining their letters. An English
  // run inside the Hindi interface, such as a matter's name as filed, must keep the day tracking: :lang(en) restores
  // it, --tr-title's -0.01em included.
  const latin = list.find((b) => b.selector === ":lang(en)");
  expect(latin).toBeDefined();
  expect(latin?.decls.size).toBeGreaterThan(0);
  for (const [token, value] of latin?.decls ?? []) expect(value, token).toBe(day.get(token));
  expect(latin?.decls.get("--tr-title")).toBe("-0.01em");
});

test("the block reader finds a rule's custom properties and skips Tailwind's mappings", () => {
  const read = blocks(
    ":root { --a: 1px; } @theme { --color-x: red; } @media (x) { .b { --c: 2; } }",
  );
  expect(read.map((b) => [b.context, b.selector, [...b.decls]])).toEqual([
    ["", ":root", [["--a", "1px"]]],
    ["@media (x)", ".b", [["--c", "2"]]],
  ]);
});
