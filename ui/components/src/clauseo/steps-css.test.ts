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

test("nothing on the line is ever cut: no nowrap meta line, no ellipsis, no clipping; the words wrap, a long name with no spaces too", () => {
  expect(steps).not.toMatch(/text-overflow/);
  expect(steps).not.toMatch(/overflow: hidden/);
  expect(steps).not.toMatch(/flex-wrap: nowrap/);
  expect(steps).not.toMatch(/step-part|step-group-kept/);
  const words = rule(".step-group-words");
  expect(words).toMatch(/overflow-wrap: anywhere;/);
  expect(words).toMatch(/min-width: 0;/);
  // on the meta line the line may shrink below its words' width, so it wraps under the name and the time and its
  // words wrap inside it
  expect(rule(".meta > .step-group-line")).toMatch(/min-width: 0;/);
  expect(rule(".step-group-line")).toMatch(/max-width: 100%;/);
});

test("when the words wrap, the icons stand on their first line; one line keeps the chip's 24 px with the words in its middle", () => {
  const line = rule(".step-group-line");
  expect(line).toMatch(/align-items: flex-start;/);
  expect(line).toMatch(/min-height: calc\(var\(--s6\) \* var\(--k\)\);/);
  expect(line).toMatch(/padding: calc\(\(var\(--s6\) - var\(--lh-small\)\) \* var\(--k\) \/ 2\)/);
  expect(rule(".step-group-icons")).toMatch(/height: calc\(var\(--lh-small\) \* var\(--k\)\);/);
});

test("an icon held to its word wraps with it, never alone at a line's start", () => {
  expect(rule(".glued")).toMatch(/white-space: nowrap;/);
});

test("the chevron takes the line's own ink wherever it stands among the words, the trouble's or the aside's too", () => {
  expect(rule(".step-group-chev")).toMatch(/color: var\(--line-ink\);/);
  const line = rule(".step-group-line");
  expect(line).toMatch(/--line-ink: var\(--muted\);/);
  expect(steps).toMatch(/@variant is-hover \{\s*--line-ink: var\(--primary\);/);
});
