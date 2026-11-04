// @vitest-environment happy-dom
// WebKit stops a keystroke's layout at a clipping box whose width and height are both set, but never at a text field
// that is a flex item (RenderObject.cpp, isLayoutBoundary). The composer's field is a flex item, so it takes a set
// width the row overrides: without it each key laid out the whole page and the window's grid repainted the room's
// centre whole (#117, WKWebView trace).
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, expect, test } from "vitest";

const sheet = readFileSync(join(import.meta.dirname, "composer.css"), "utf8");

afterEach(() => {
  document.head.innerHTML = "";
  document.body.innerHTML = "";
});

test("the field has a set width and takes its real width from the row", () => {
  const plain = sheet.match(/\.room-field textarea \{[^}]*\}/)?.[0] ?? "";
  const style = document.createElement("style");
  style.textContent = `:root { --s0-5: 2px; --lh-field: 26px; --k: 1; } ${plain.replace(/@apply [^;]+;/, "")}`;
  document.head.append(style);
  document.body.innerHTML = `<div class="room-field"><textarea></textarea></div>`;
  const area = document.querySelector("textarea");
  if (area === null) throw new Error("no field");
  const cs = getComputedStyle(area);
  expect(cs.width).toBe("0px");
  // the width is never used: the row's flex sizes the field from a basis of 0, and nothing holds it wider
  expect(cs.flexBasis).toBe("0%");
  expect(cs.flexGrow).toBe("1");
  expect(parseFloat(cs.minWidth)).toBe(0);
});
