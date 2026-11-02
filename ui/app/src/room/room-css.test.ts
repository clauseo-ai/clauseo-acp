// The room's stylesheet (styles/index.css) carries the sheet of every part the room draws: the rail and its rows
// (matters.css: the find field, the moved folder's ring, the current matter), and the new matter form
// (matter-form.css: the folder line with Change, the placeholder's tone). Without them the room build draws those
// parts unstyled, as the WebKit shots of U2 showed.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";

const sheet = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "../styles/index.css"),
  "utf8",
);
const imported = [...sheet.matchAll(/@import "@clauseo\/components\/clauseo\/([\w-]+\.css)"/g)].map(
  (m) => m[1],
);

test("the room's stylesheet imports the sheets of the rail and the new matter form, in parts.css's order", () => {
  for (const part of ["shell.css", "matter-form.css", "matters.css"])
    expect(imported).toContain(part);
  // parts.css lays matter-form.css before matters.css, both after live.css
  expect(imported.indexOf("live.css")).toBeLessThan(imported.indexOf("matter-form.css"));
  expect(imported.indexOf("matter-form.css")).toBeLessThan(imported.indexOf("matters.css"));
});

test("the room's stylesheet styles the permission prompt's confirming line, never leaning on edge.css", () => {
  expect(imported).toContain("live.css");
  const live = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../../../components/src/clauseo/live.css"),
    "utf8",
  );
  expect(live).toMatch(/\.perm-confirming\s*\{/);
  expect(live).toMatch(/\.perm\[data-confirming\] \.perm-actions\s*\{/);
});
