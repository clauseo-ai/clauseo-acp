// isolate.ts: control and bidi formatting characters go, and the rest is wrapped in FSI … PDI, as the native dialogs'
// isolate does (crates/desktop/src/dialogs/words.rs).
import { expect, test } from "vitest";
import { isolate } from "./isolate.ts";

const FSI = "\u2068";
const PDI = "\u2069";

test("plain ASCII is unchanged but for the wrap", () => {
  expect(isolate("/Users/meera/Documents/Precedents")).toBe(
    `${FSI}/Users/meera/Documents/Precedents${PDI}`,
  );
  expect(isolate("")).toBe(`${FSI}${PDI}`);
});

test("a right-to-left override inside a path goes", () => {
  expect(isolate("C:\\Cases\\\u202Efdp.exe")).toBe(`${FSI}C:\\Cases\\fdp.exe${PDI}`);
});

test("a line break, a carriage return, a tab and the other Cc characters go", () => {
  expect(isolate("Precedents\nAllowed.")).toBe(`${FSI}PrecedentsAllowed.${PDI}`);
  expect(isolate("a\r\tb\u0000c\u001Bd\u007Fe\u0085f\u009Fg")).toBe(`${FSI}abcdefg${PDI}`);
});

test("an isolate of its own can't close the wrap early, and every bidi mark goes", () => {
  expect(isolate(`${FSI}x${PDI}y`)).toBe(`${FSI}xy${PDI}`);
  expect(isolate("a\u061Cb\u200Ec\u200Fd\u202Ae\u202Bf\u202Cg\u202Dh\u2066i\u2067j")).toBe(
    `${FSI}abcdefghij${PDI}`,
  );
});

test("other text stays, Hindi and Arabic letters and ZWJ and ZWNJ included", () => {
  expect(isolate("मामला عقد a\u200Db\u200Cc")).toBe(`${FSI}मामला عقد a\u200Db\u200Cc${PDI}`);
});
