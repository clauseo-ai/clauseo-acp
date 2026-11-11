import { expect, test } from "vitest";
import { sharedEnd, sharedStart } from "./composer-diff.ts";
import { insertedOnly } from "./composer-edit.ts";

// past one 4096-character block, and short of one, the parting is found to the character
const LONG = "Clause 00001. The parties agree.\n".repeat(400);

test("sharedStart finds where two values part, inside a block, past a block, or at the shorter's end", () => {
  expect(sharedStart("", "")).toBe(0);
  expect(sharedStart("abc", "abx")).toBe(2);
  expect(sharedStart(LONG, `${LONG}x`)).toBe(LONG.length);
  for (const at of [0, 1, 4095, 4096, 4097, 9000, LONG.length - 1]) {
    const next = `${LONG.slice(0, at)}#${LONG.slice(at)}`;
    expect(sharedStart(LONG, next)).toBe(at);
  }
  // never past max
  expect(sharedStart(LONG, LONG, 5000)).toBe(5000);
});

test("sharedEnd finds how much two values share at their ends, up to max", () => {
  expect(sharedEnd("abc", "xbc")).toBe(2);
  expect(sharedEnd(LONG, `x${LONG}`)).toBe(LONG.length);
  for (const at of [0, 1, 4095, 4096, 4097, 9000, LONG.length - 1]) {
    const next = `${LONG.slice(0, at)}#${LONG.slice(at)}`;
    expect(sharedEnd(LONG, next)).toBe(LONG.length - at);
  }
  expect(sharedEnd(LONG, LONG, 10)).toBe(10);
});

test("insertedOnly across blocks: words put in at one place, or anything taken out", () => {
  expect(insertedOnly(LONG, `${LONG.slice(0, 5000)}new words${LONG.slice(5000)}`)).toBe(true);
  expect(insertedOnly(LONG, `${LONG}x`)).toBe(true);
  expect(insertedOnly(LONG, LONG)).toBe(true);
  expect(insertedOnly(LONG, `${LONG.slice(0, 5000)}X${LONG.slice(5001)}`)).toBe(false);
  expect(insertedOnly(LONG, LONG.slice(1))).toBe(false);
});
