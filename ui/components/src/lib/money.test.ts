// The money rule (money.ts): the rupee sign, Indian grouping, Western digits, decimals only for a paise remainder.
import { expect, test } from "vitest";
import { count, rupees } from "./money.ts";

const DEVANAGARI_DIGITS = /[\u0966-\u096f]/;

test("whole rupees have no decimals, in Indian grouping", () => {
  expect(rupees(0)).toBe("₹0");
  expect(rupees(100)).toBe("₹1");
  expect(rupees(23_600)).toBe("₹236");
  expect(rupees(1_18_000)).toBe("₹1,180");
  expect(rupees(1_18_00_000)).toBe("₹1,18,000");
  // the brief's example reads "10,00,00,00,000 is ₹1,00,00,000", a slip by a factor of ten: a crore of rupees is
  // 1,00,00,00,000 paise
  expect(rupees(1_00_00_00_000)).toBe("₹1,00,00,000");
  expect(rupees(10_00_00_00_000)).toBe("₹10,00,00,000");
});

test("a paise remainder shows two decimals", () => {
  expect(rupees(2_124)).toBe("₹21.24");
  expect(rupees(5)).toBe("₹0.05");
  expect(rupees(50)).toBe("₹0.50");
  expect(rupees(12_34_56_789)).toBe("₹12,34,567.89");
});

test("counts take the same grouping without a sign", () => {
  expect(count(0)).toBe("0");
  expect(count(180)).toBe("180");
  expect(count(10_000)).toBe("10,000");
  expect(count(10_00_000)).toBe("10,00,000");
});

// the formatters take no language: a Hindi page writes money exactly as an English one does
test("Western digits and no Rs., in either language", () => {
  for (const s of [rupees(23_600), rupees(2_124), rupees(1_18_00_000), count(10_00_000)]) {
    expect(s).not.toMatch(DEVANAGARI_DIGITS);
    expect(s).not.toContain("Rs");
    expect(s).toMatch(/^[₹0-9.,]+$/);
  }
});
