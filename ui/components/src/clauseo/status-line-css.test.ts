// The status line's model and effort must show, not merely be in the DOM: a slip in composer.css once put the bare
// `.room-status-menu` into a `display: none` rule's selector list (meant for its lead dot alone), and every DOM test
// still passed, since happy-dom computes no stylesheet. This reads the sheet's rules and fails on any rule that hides
// one of the line's parts by its own class.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

const CSS = readFileSync(new URL("./composer.css", import.meta.url), "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);
/** The status line's parts that must stand: each model and effort menu, its trigger, and the tool line. */
const PARTS = [".room-status-menu", ".room-pick", ".room-status-tools"];
const HIDES =
  /display\s*:\s*none|visibility\s*:\s*hidden|(?:^|[;\s])(?:width|height)\s*:\s*0(?:px)?\s*(?:;|$)/;

test("no rule in composer.css hides the status line's model and effort menus by their own class", () => {
  const hiding = [...CSS.matchAll(/([^{}]+)\{([^{}]*)\}/g)]
    .filter(([, , body]) => HIDES.test(body!))
    .flatMap(([, selectors]) => selectors!.split(",").map((s) => s.trim()))
    .filter((s) => PARTS.includes(s));
  expect(hiding).toEqual([]);
});
