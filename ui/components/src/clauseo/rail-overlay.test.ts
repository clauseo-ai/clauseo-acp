// In one column (responsive.css, under 1000 px) Matters opens the list as a panel over the work, as the folded rail
// does from 1000 to 1199 px: fixed under the toolbar, its list scrolling inside it, so nothing under it moves and the
// page grows by nothing (D-061: at 960 px the list ran inline and pushed the work below the fold). The composer stays
// docked under it. happy-dom lays nothing out, so the rules are read from the stylesheet; the screenshots at 760, 960
// and 1100 px are in the PR, and the open and close are room/rail-overlay.test.tsx's.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

const css = readFileSync(new URL("responsive.css", import.meta.url), "utf8");

/** The declarations of `selector`'s rule in `text`, or null where there is none. */
function rule(text: string, selector: string): string | null {
  const at = text.indexOf(`${selector} {`);
  if (at === -1) return null;
  const open = text.indexOf("{", at);
  return text.slice(open + 1, text.indexOf("}", open));
}

/** The first `@variant max-mid` block: the one-column room. */
function firstMaxMid(): string {
  const at = css.indexOf("@variant max-mid {");
  expect(at).toBeGreaterThan(-1);
  return css.slice(at, css.indexOf("@variant", at + 1));
}

/** The `@variant rail-folded` block: the folded rail's panel from 1000 to 1199 px. */
function folded(): string {
  const at = css.indexOf("@variant rail-folded {");
  expect(at).toBeGreaterThan(-1);
  return css.slice(at, css.indexOf("@variant", at + 1));
}

const z = (block: string | null) => Number(/z-index: (\d+);/.exec(block ?? "")?.[1]);

test("in one column the composer docks to the window's bottom", () => {
  const dock = rule(firstMaxMid(), ".room-composer");
  expect(dock).toMatch(/position: sticky;/);
  expect(dock).toMatch(/bottom: 0;/);
});

test("in one column the list is a panel over the work, shown only while Matters is open", () => {
  const narrow = firstMaxMid();
  const rail = rule(narrow, ".room-rail");
  expect(rail).toMatch(/display: none;/);
  // out of the page's flow: it pushes nothing down and adds nothing to the page's height
  expect(rail).toMatch(/position: fixed;/);
  // under the toolbar, whose height the room keeps on the window, never over it
  expect(rail).toMatch(/top: var\(--top-h, var\(--toolbar\)\);/);
  expect(rail).toMatch(/bottom: calc\(var\(--s3\)/);
  // the folded rail's width, never wider than the window
  expect(rail).toMatch(
    /width: min\(var\(--rail-panel-w\), calc\(var\(--view-w\) - 2 \* var\(--s3\)\)\);/,
  );
  // the list scrolls inside the panel, and its scroll doesn't run on into the page's
  expect(rail).toMatch(/overflow-y: auto;/);
  expect(rail).toMatch(/overscroll-behavior: contain;/);
  expect(rule(narrow, ".room-rail[data-open]")).toMatch(/display: flex;/);
});

test("the panel stands over the docked composer and the pinned question, as high as the folded rail's", () => {
  const narrow = firstMaxMid();
  const rail = z(rule(narrow, ".room-rail"));
  expect(rail).toBeGreaterThan(z(rule(narrow, ".room-composer")));
  expect(rail).toBeGreaterThan(z(rule(narrow, ".room-pin")));
  expect(rail).toBe(z(rule(folded(), ".room-rail")));
  // the panel's own look is the folded rail's
  for (const look of ["background: var(--ground);", "box-shadow: var(--e3);"]) {
    expect(rule(narrow, ".room-rail")).toContain(look);
    expect(rule(folded(), ".room-rail")).toContain(look);
  }
});

test("opening Matters moves nothing under the panel: no rule un-docks the composer or reflows the work", () => {
  expect(css).not.toContain(":has(> .room-rail[data-open])");
});
