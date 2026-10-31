import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import * as names from "./token-names.ts";

const byName = (a: string | undefined, b: string | undefined) => (a ?? "").localeCompare(b ?? "");

// cn() merges classes by the names tokens-theme.css's @theme gives each namespace; the two lists must agree.
const css = readFileSync(new URL("../styles/tokens-theme.css", import.meta.url), "utf8");
const theme = css.slice(css.indexOf("@theme inline reference"));
const keys = (ns: string) => [
  ...new Set(
    [...theme.matchAll(new RegExp(`^\\s*--${ns}-([a-z0-9-]+?)(?:--line-height)?:`, "gm"))].map(
      (m) => m[1],
    ),
  ),
];

test.each([
  ["color", names.color],
  ["text", names.text],
  ["font-weight", names.fontWeight],
  ["tracking", names.tracking],
  ["leading", names.leading],
  ["spacing", names.spacing],
  ["radius", names.radius],
  ["shadow", names.shadow],
  ["ease", names.ease],
] as const)("%s", (ns, list) => {
  expect([...list].sort(byName)).toEqual(keys(ns).sort(byName));
});

test("font families", () => {
  expect([...names.font].sort(byName)).toEqual(
    keys("font")
      .filter((k) => !k?.startsWith("weight"))
      .sort(byName),
  );
});
