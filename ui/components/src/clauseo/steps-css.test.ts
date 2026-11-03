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
  // 2 px between rows, the work list's rhythm
  expect(rows).toMatch(/gap: var\(--s0-5\);/);
});

test("a failure on the folded line is in primary ink at the line's own weight, the prototype chip's 450, never bolder", () => {
  const trouble = rule(".step-group-trouble");
  expect(trouble).toMatch(/color: var\(--primary\);/);
  expect(trouble).toMatch(/font-weight: var\(--w-ui\);/);
  expect(steps).not.toMatch(/font-weight: (5|6|7)\d\d/);
});

test("the live group's line is muted like the folded one, its count in --muted, never in primary ink", () => {
  expect(rule(".step-group-line")).toMatch(/text-muted/);
  expect(steps).not.toMatch(/\[data-live\] \.step-group-now/);
  expect(rule(".step-group[data-live] .step-group-more")).toMatch(/color: var\(--muted\);/);
  // primary ink is a failure's alone: no rule gives it to the line's words
  expect(steps).not.toMatch(/\.step-group-now \{[^}]*--primary/);
});

test("the folded line's hover eases its well and ink over --dur-quick on the arrive curve, as the prototype's chip does", () => {
  const line = rule(".step-group-line");
  expect(line).toMatch(/background-color var\(--dur-quick\) var\(--ease-arrive\)/);
  expect(line).toMatch(/color var\(--dur-quick\) var\(--ease-arrive\)/);
  // the chevron still turns at once
  expect(steps).not.toMatch(/\.step-group-chev[^}]*transition/);
});

test("on the answer's meta line the line never wraps to a row of its own: the meta line keeps one line, the words cut with an ellipsis, the name and the time keep their width", () => {
  expect(rule(".meta:has(> .step-group-line)")).toMatch(/flex-wrap: nowrap;/);
  expect(rule(".meta > :not(.step-group-line)")).toMatch(/flex: none;/);
  const line = rule(".meta > .step-group-line");
  expect(line).toMatch(/flex: 0 1 auto;/);
  expect(line).toMatch(/min-width: 0;/);
  const words = rule(".meta > .step-group-line .step-group-words");
  expect(words).toMatch(/white-space: nowrap;/);
  expect(words).toMatch(/overflow: hidden;/);
  expect(words).toMatch(/text-overflow: ellipsis;/);
  expect(rule(".meta > .step-group-line .step-group-chev")).toMatch(/flex: none;/);
});

test("a sentence dropped to fit the meta line is out of sight but stays in the button's name", () => {
  expect(rule(".step-part-dropped")).toMatch(/@apply sr-only;/);
  // the "…" in no text, unseen and out of the flow until wanted
  expect(rule(".step-part-cut::after")).toMatch(/content: " …";/);
  expect(rule(".step-part-cut:not([data-cut])")).toMatch(/position: absolute;/);
  expect(rule(".step-part-cut:not([data-cut])")).toMatch(/visibility: hidden;/);
});
