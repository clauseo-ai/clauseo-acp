// hasBackslash (folder-backslash.ts) on each OS: a backslash in a folder's name warns on macOS and Linux, never on
// Windows, where it is the separator; a plain path warns nowhere.
import { expect, test } from "vitest";
import { hasBackslash } from "./folder-backslash.ts";

test.each([
  ["mac", "Documents/Clauseo\\ Test\\ Matter", true],
  ["mac", "/Volumes/Work/a\\b", true],
  ["mac", "Documents/Clauseo Test Matter", false],
  ["linux", "Documents/Clauseo\\ Test\\ Matter", true],
  ["linux", "/srv/matters/Acme", false],
  ["windows", "Documents\\Acme", false],
  ["windows", "D:\\Matters\\Clauseo\\ Test", false],
  ["windows", "D:/Matters/Acme", false],
] as const)("on %s, %s: %s", (platform, path, warns) => {
  expect(hasBackslash(path, platform)).toBe(warns);
});
