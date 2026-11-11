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

/** A span of 18 px lines, clamped to two as wall.css clamps it, 20 letters a line. `extra` is the
 * px a script's taller glyph box adds to the measured height, and `lineHeight` is what the computed
 * style reports. */
function tall(extra: number, lineHeight: number = 18): Fitted {
  let text = "";
  return {
    get textContent() {
      return text;
    },
    set textContent(value: string | null) {
      text = value ?? "";
    },
    get scrollHeight() {
      return Math.max(1, Math.ceil(text.length / 20)) * 18 + extra;
    },
    get clientHeight() {
      return Math.min(2, Math.max(1, Math.ceil(text.length / 20))) * 18;
    },
    lineHeight,
  };
}

test("a one-line name whose span measures 2 px taller than its box stands whole", () => {
  const el = tall(2);
  expect(fit(el, "Plaint.pdf")).toBe(false);
  expect(el.textContent).toBe("Plaint.pdf");
});

test("a one-line Hindi name in a taller glyph box stands whole", () => {
  const el = tall(2);
  expect(fit(el, "अनुबंध.pdf")).toBe(false);
  expect(el.textContent).toBe("अनुबंध.pdf");
});

test("a name that needs a third line is still cut from its middle, keeping its extension", () => {
  const el = tall(2);
  const name = "Statement of claim with annexures A to F, final.pdf";
  expect(fit(el, name)).toBe(true);
  expect(el.textContent).toBe("Statement of claim with annex… final.pdf");
});

test("a line height that is not a number (normal) keeps the 1 px slack", () => {
  expect(fit(tall(2, Number.NaN), "अनुबंध.pdf")).toBe(true);
  expect(fit(tall(1, Number.NaN), "अनुबंध.pdf")).toBe(false);
});
