// @vitest-environment happy-dom
// The composer's words keep clear of the well's rounded edge: with no Add papers before them, the field gives them
// their own left padding; with Add papers there, the button holds that space and the padding stays small.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, expect, test } from "vitest";

const sheet = readFileSync(join(import.meta.dirname, "composer.css"), "utf8");
/** The two rules that set the field's left padding, as plain CSS, with the token's value. */
function rules(): string {
  const plain = sheet.match(/\.room-field textarea \{[^}]*\}/)?.[0] ?? "";
  const bare =
    sheet.match(/\.room-field:not\(:has\(\.room-field-add\)\) textarea \{[^}]*\}/)?.[0] ?? "";
  return `:root { --s0-5: 2px; --s2-5: 10px; } ${plain.replace(/@apply [^;]+;/, "")} ${bare}`;
}

function field(withAdd: boolean) {
  const style = document.createElement("style");
  style.textContent = rules();
  document.head.append(style);
  document.body.innerHTML = `<div class="room-field">${withAdd ? '<button class="room-field-add"></button>' : ""}<textarea></textarea></div>`;
  const area = document.querySelector("textarea");
  if (area === null) throw new Error("no field");
  return getComputedStyle(area).paddingLeft;
}

afterEach(() => {
  document.head.innerHTML = "";
  document.body.innerHTML = "";
});

test("with no Add papers, the words stand 10 px into the field", () => {
  expect(field(false)).toBe("10px");
});

test("with Add papers before them, the words keep the field's small padding", () => {
  expect(field(true)).toBe("2px");
});
