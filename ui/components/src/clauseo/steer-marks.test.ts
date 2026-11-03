import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

const matters = readFileSync(new URL("matters.css", import.meta.url), "utf8");
const rule = (selector: string) => {
  const at = matters.indexOf(`\n  ${selector} {`);
  expect(at, selector).toBeGreaterThan(-1);
  return matters.slice(at, matters.indexOf("}", at));
};

test("the steer note and its list, brought into view, both stand clear of the composer over the thread's foot", () => {
  const margin =
    /scroll-margin-bottom: calc\(var\(--composer-h, var\(--composer-h-rest\)\) \+ var\(--s3\)\);/;
  expect(rule(".steer-note")).toMatch(margin);
  expect(rule(".steer-note-list")).toMatch(margin);
});
