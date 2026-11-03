// The step group's opened rows stand set into the well, inset from the answer's edge, as the design's work list does
// (design/03-system thread.css .work-list), so they read as the work behind the answer, not as more of its lines.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

const steps = readFileSync(new URL("steps.css", import.meta.url), "utf8");
const rule = (selector: string) => {
  const at = steps.indexOf(`\n  ${selector} {`);
  expect(at, selector).toBeGreaterThan(-1);
  return steps.slice(at, steps.indexOf("}", at));
};

test("the opened rows sit on the well, inset by the work list's padding and corners", () => {
  const rows = rule(".step-group-rows");
  expect(rows).toMatch(/background: var\(--well\);/);
  expect(rows).toMatch(/padding: var\(--s2\) var\(--s3\);/);
  expect(rows).toMatch(/border-radius: var\(--r-control\);/);
  expect(rows).toMatch(/margin: var\(--s2\) 0 var\(--s1\);/);
});

test("a failure on the folded line is in primary ink at the line's own weight, the prototype chip's 450, never bolder", () => {
  const trouble = rule(".step-group-trouble");
  expect(trouble).toMatch(/color: var\(--primary\);/);
  expect(trouble).toMatch(/font-weight: var\(--w-ui\);/);
  expect(steps).not.toMatch(/font-weight: (5|6|7)\d\d/);
});
