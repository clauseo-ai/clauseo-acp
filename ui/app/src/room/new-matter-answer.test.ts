// Where New matter's Existing folder opens the picker (existingStart): Documents the first time in a window, the last
// folder picked after. Pure; nothing on disk.
import { expect, test } from "vitest";
import type { Matters } from "./matters.ts";
import { existingStart } from "./new-matter-answer.ts";

test("Documents the first time in a window, the last folder picked after; another window starts again", () => {
  const one = {} as Matters;
  const two = {} as Matters;
  expect(existingStart(one)).toBe("documents");
  expect(existingStart(one)).toBe("last");
  expect(existingStart(two)).toBe("documents");
  expect(existingStart(one)).toBe("last");
});
