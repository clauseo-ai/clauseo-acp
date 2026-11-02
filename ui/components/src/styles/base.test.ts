import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { blocks } from "./css-blocks.ts";

const read = (file: string) => readFileSync(new URL(`./${file}`, import.meta.url), "utf8");

// The first frame, before display.ts sets data-theme: a dark OS gets night's ground, not day's, so a night user sees
// no light flash. The value is spelt out (a var() there would read :root's day value), so it must stay night's
// --ground, which this holds it to.
test("before the light is set, a dark OS starts on night's ground", () => {
  const night = blocks(read("tokens-colour.css")).find(
    (b) => b.selector.includes('[data-theme="night"]') && b.decls.has("--ground"),
  );
  expect(night).toBeDefined();
  const base = read("base.css");
  const media =
    /@media \(prefers-color-scheme: dark\) \{\s*html:not\(\[data-theme\]\) \{\s*background: ([^;]+);/.exec(
      base,
    );
  expect(media?.[1]).toBe(night?.decls.get("--ground"));
});
