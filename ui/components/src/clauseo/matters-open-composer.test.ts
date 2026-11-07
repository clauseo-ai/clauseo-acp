// In one column (responsive.css, under 1000 px) the composer docks to the window's bottom, and while Matters is open it
// doesn't: at 640 by 480 the docked sign-in card covered the matters list (the day-one walkthrough on a Windows console).
// happy-dom lays nothing out, so the rule is read from the stylesheet; the screenshots at 640 by 480 and 800 by 600, in
// English and in Hindi at text size 20, are in the PR.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

const css = readFileSync(new URL("responsive.css", import.meta.url), "utf8");
const OPEN = ".room-window:has(> .room-rail[data-open]) .room-composer";

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

test("in one column the composer docks to the window's bottom", () => {
  const dock = rule(firstMaxMid(), ".room-composer");
  expect(dock).toMatch(/position: sticky;/);
  expect(dock).toMatch(/bottom: 0;/);
});

test("while Matters is open the composer follows the work instead, so it covers none of the list", () => {
  const narrow = firstMaxMid();
  expect(rule(narrow, OPEN)).toMatch(/position: static;/);
  // after the dock, so it wins at the same layer
  expect(narrow.indexOf(`${OPEN} {`)).toBeGreaterThan(narrow.indexOf(".room-composer {"));
});

test("only the one-column room changes: no other rule un-docks the composer", () => {
  expect(css.split(OPEN).length).toBe(2);
});
