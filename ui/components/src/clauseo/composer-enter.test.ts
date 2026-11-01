// G1a's Enter rule for the composer's field: Enter sends, Shift+Enter breaks the line, and neither while an input
// method composes, whether the browser says so with isComposing or with keyCode 229.
import { expect, test } from "vitest";
import { sendsOnEnter } from "./composer-keys.ts";

const key = (over: Partial<Parameters<typeof sendsOnEnter>[0]>) => ({
  key: "Enter",
  shiftKey: false,
  isComposing: false,
  keyCode: 13,
  ...over,
});

test("Enter sends", () => {
  expect(sendsOnEnter(key({}))).toBe(true);
});

test("Shift+Enter breaks the line and sends nothing", () => {
  expect(sendsOnEnter(key({ shiftKey: true }))).toBe(false);
});

test("the Enter that commits an input method's candidate sends nothing", () => {
  expect(sendsOnEnter(key({ isComposing: true }))).toBe(false);
  expect(sendsOnEnter(key({ keyCode: 229 }))).toBe(false);
});

test("any other key sends nothing", () => {
  expect(sendsOnEnter(key({ key: "a", keyCode: 65 }))).toBe(false);
});
