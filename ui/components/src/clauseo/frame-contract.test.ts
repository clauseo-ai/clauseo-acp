// The frame contract (responsive.css): where the window draws its own frame, the toolbar keeps the window's top and the
// parts that line up with the host's native ones (the caption buttons' hit-test, macOS's traffic lights) never move.
// Each rule is read from the stylesheet, so dropping one fails here.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

const css = readFileSync(new URL("responsive.css", import.meta.url), "utf8");
const OWN = ':root[data-frame="own"]';

/** The declarations of `selector`'s rule in `text`, or null where there is none. */
function rule(text: string, selector: string): string | null {
  const at = text.indexOf(`${selector} {`);
  if (at === -1) return null;
  const open = text.indexOf("{", at);
  return text.slice(open + 1, text.indexOf("}", open));
}

/** The text of the last `@variant max-mid` block: the frame contract's narrow rules. */
function lastMaxMid(): string {
  const at = css.lastIndexOf("@variant max-mid {");
  expect(at).toBeGreaterThan(-1);
  return css.slice(at);
}

test("outside any variant, the toolbar sticks to the window's top over the pinned question, on the ground", () => {
  const head = css.slice(0, css.indexOf("@variant"));
  const top = rule(head, `${OWN} .room-top`);
  expect(top).toMatch(/position: sticky;/);
  expect(top).toMatch(/top: 0;/);
  // above .room-pin's z-index 3
  expect(Number(/z-index: (\d+);/.exec(top ?? "")?.[1])).toBeGreaterThan(3);
  expect(top).toMatch(/background: var\(--ground\);/);
  expect(rule(head, `${OWN} .room-window`)).toMatch(/min-height: 0;/);
});

test("in one column the pinned question and what scrolls into view stand below the toolbar", () => {
  const narrow = lastMaxMid();
  expect(rule(narrow, `${OWN} .room-pin`)).toMatch(/top: var\(--top-h, var\(--toolbar\)\);/);
  expect(rule(narrow, `${OWN} .step-question`)).toMatch(/scroll-margin-top: calc\(var\(--top-h/);
  expect(rule(narrow, `${OWN} .room-window :is(a[href], button, textarea, [tabindex])`)).toMatch(
    /scroll-margin-top: calc\(var\(--top-h/,
  );
});

test("in one column, at every narrow width, the native lights keep their wide box", () => {
  const narrow = lastMaxMid();
  const lights = rule(narrow, `${OWN} .room-lights[data-native]`);
  // shown under max-narrow too, which hides painted lights: this block comes after it
  expect(css.lastIndexOf("@variant max-mid {")).toBeGreaterThan(
    css.indexOf("@variant max-narrow {"),
  );
  expect(lights).toMatch(/display: flex;/);
  expect(lights).toMatch(/align-self: flex-start;/);
  expect(lights).toMatch(/margin-top: calc\(\(var\(--lead-h\) - var\(--s3\)\) \/ 2 - var\(--s3\)/);
  expect(lights).toMatch(/margin-left: calc\(var\(--s5\) - var\(--s4\)/);
});

test("in one column, with no search field, Matters pushes the caption buttons to the window's right", () => {
  expect(rule(lastMaxMid(), `${OWN} .room-top:not(:has(.room-search)) .room-rail-toggle`)).toMatch(
    /margin-left: auto;/,
  );
});
