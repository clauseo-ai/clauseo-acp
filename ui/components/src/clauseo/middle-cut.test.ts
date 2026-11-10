// The middle cut of a file's name (middle-cut.ts), over a span whose height grows with its text: two lines of 20
// letters each fit, so a longer name keeps as much of its head as fits, an ellipsis, and its last letters with the
// extension.
import { expect, test } from "vitest";
import { fit, nameParts, type Fitted } from "./middle-cut.ts";

/** A span 20 letters wide and two lines tall. */
function span(): Fitted {
  let text = "";
  return {
    get textContent() {
      return text;
    },
    set textContent(value: string | null) {
      text = value ?? "";
    },
    get scrollHeight() {
      return Math.max(1, Math.ceil(text.length / 20)) * 10;
    },
    clientHeight: 20,
  };
}

test("a name that fits its two lines stands whole", () => {
  const el = span();
  expect(fit(el, "Notice to the bank.docx")).toBe(false);
  expect(el.textContent).toBe("Notice to the bank.docx");
});

test("a longer name loses letters from its middle and keeps its extension, as much of its head as fits", () => {
  const el = span();
  const name = "Statement of claim with annexures A to F, final.pdf";
  expect(fit(el, name)).toBe(true);
  expect(el.textContent).toBe("Statement of claim with annex… final.pdf");
});

test("the tail is the extension and the six letters before it; no extension, the last six", () => {
  expect(nameParts("Invoices listed.csv")).toEqual({ head: "Invoices ", tail: "listed.csv" });
  expect(nameParts("README")).toEqual({ head: "", tail: "README" });
});
