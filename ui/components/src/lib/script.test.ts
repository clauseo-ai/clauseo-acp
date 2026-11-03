import { expect, test } from "vitest";
import { langOfText } from "./script.ts";

test("text whose letters are mostly Devanagari is Hindi", () => {
  expect(langOfText("किरायेदार को तीस दिन का नोटिस देना होगा।")).toBe("hi");
  expect(langOfText("धारा 8.3")).toBe("hi");
});

test("English text, and text with no letter, is not", () => {
  expect(langOfText("The tenant must give thirty days' notice.")).toBeUndefined();
  expect(langOfText("")).toBeUndefined();
  expect(langOfText("1234 5.6 ,;-")).toBeUndefined();
  // Devanagari digits and the danda are not letters
  expect(langOfText("१२३४ । ॥")).toBeUndefined();
});

test("mixed text follows the majority of its letters", () => {
  // 10 Devanagari letters with their marks against 5 Latin
  expect(langOfText("Acme v. किरायेदार नोटिस")).toBe("hi");
  expect(langOfText("Under the Rent Control Act, the किरायेदार must give notice.")).toBeUndefined();
  // half is not a majority
  expect(langOfText("ab कख")).toBeUndefined();
});

test("a mark counts with the letter it sits on", () => {
  // three Devanagari letters, each with a vowel sign: six of nine against three Latin
  expect(langOfText("abc किको")).toBe("hi");
  // a combining acute on a Latin letter counts as Latin: two Latin letters with their marks are four
  expect(langOfText("e\u0301 कखग")).toBe("hi");
  expect(langOfText("e\u0301e\u0301 कखग")).toBeUndefined();
  // a mark after no letter counts for nothing
  expect(langOfText(" \u093f\u093f\u093f a")).toBeUndefined();
});

test("Devanagari Extended counts; an astral letter at the end is a letter (#248)", () => {
  expect(langOfText("\ua8f2\ua8f3\ua8fb a")).toBe("hi");
  expect(langOfText("क 𝐀𝐁")).toBeUndefined();
  expect(langOfText("कख 𝐀")).toBe("hi");
});

test("it is fast: a long answer's worth of text in a few milliseconds", () => {
  const text = "किरायेदार को Acme Traders तीस दिन का नोटिस देना होगा। ".repeat(4000); // about 200k characters
  langOfText(text);
  const start = performance.now();
  expect(langOfText(text)).toBe("hi");
  expect(performance.now() - start).toBeLessThan(100);
});
