// UD1, the UI review's narrow-window and text-size defects, held where happy-dom can hold them (it lays nothing out): the
// first screen's column and the install line's buttons may narrow inside the well, the gate's quiet buttons and the
// first screen's buttons wrap their words (as utilities, which a component rule can't beat), and the foot keeps the
// welcome's scrollbar gutter. Checked by eye at 320 px and text size 20 on the review page too (impl-report.md).
// @vitest-environment happy-dom
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "vitest";
import { CLAUDE, READY, button, byId, install, mountFirstRun, signIn } from "./rig.tsx";

const sheet = readFileSync(
  join(import.meta.dirname, "../../../../components/src/clauseo/first-launch/first-launch.css"),
  "utf8",
);
/** The declarations of one rule of the sheet, by its exact selector. */
const rule = (selector: string) => {
  const at = sheet.indexOf(`${selector} {`);
  return at < 0 ? "" : sheet.slice(at, sheet.indexOf("}", at));
};
const wraps = (el: Element | null) => {
  const classes = el?.className.split(/\s+/) ?? [];
  return (
    classes.includes("whitespace-normal") &&
    classes.includes("h-auto") &&
    !classes.includes("whitespace-nowrap")
  );
};

test("the first screen's column is one track no word can widen: the lede and fine print stay inside the well", () => {
  expect(rule(".fl-ask")).toContain("grid-template-columns: minmax(0, 1fr)");
  expect(rule(".fl-open")).toContain("grid-template-columns: minmax(0, 1fr)");
});

test("the first screen's buttons wrap their words rather than run past a narrow column", async () => {
  await mountFirstRun({ matters: [] });
  expect(wraps(byId("open-folder"))).toBe(true);
  expect(byId("open-folder")?.className).toContain("max-w-full");
  expect(wraps(byId("start-new-matter"))).toBe(true);
});

test("the install line's buttons may narrow and wrap inside the well", () => {
  const acts = rule(".working-acts:has(> .fl-install)");
  expect(acts).toContain("min-width: 0");
  expect(acts).toContain("max-width: 100%");
  expect(rule(".fl-install")).toContain("flex-wrap: wrap");
});

test("the gate's quiet buttons wrap their words: the button's nowrap gives way", async () => {
  await mountFirstRun({ first: [install(CLAUDE, READY), signIn(CLAUDE, { signed_out: {} })] });
  expect(wraps(button("Use ChatGPT instead"))).toBe(true);
  expect(wraps(button("Use an API key or my firm's account"))).toBe(true);
});

test("the foot keeps the welcome's scrollbar gutter, so the working line shares the button's edge", () => {
  expect(rule(".fl-welcome")).toContain("scrollbar-gutter: stable");
  expect(rule(".fl-foot")).toContain("scrollbar-gutter: stable");
  expect(rule(".fl-foot")).toContain("overflow-y: auto");
});

test("a key or code field in the gate draws no focus ring of its own: the composer's well draws the one ring", () => {
  expect(sheet).not.toContain(".fl-gate-input:focus-visible");
  expect(rule(".fl-gate-input")).toContain("outline: none");
});
