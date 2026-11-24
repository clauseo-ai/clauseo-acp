// Where New matter's Existing folder opens the picker (existingStart): Documents the first time in this window, the last
// folder picked after. Pure; nothing on disk.
import { expect, test } from "vitest";
import { existingStart } from "./new-matter-answer.ts";

test("Documents the first time, the last folder picked after", () => {
  expect(existingStart()).toBe("documents");
  expect(existingStart()).toBe("last");
  expect(existingStart()).toBe("last");
});
