// folder-backslash.ts on each OS: a backslash in a folder's name warns on macOS and Linux, never on Windows, where it
// is the separator; a plain path warns nowhere. backslashAt names the folder that has it: the folder's own name, or
// the nearest folder above it whose name has one; backslashPlace says it as the refusal's line takes it.
import { expect, test } from "vitest";
import { backslashAt, backslashPlace, hasBackslash } from "./folder-backslash.ts";

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

test.each(["mac", "linux"] as const)(
  "on %s, the folder's own name, or the nearest folder above it whose name has a backslash",
  (platform) => {
    expect(backslashAt("Documents/Clauseo\\ Test\\ Matter", platform)).toEqual({ self: true });
    expect(backslashAt("Documents/a\\b/Acme\\Sons/", platform)).toEqual({ self: true });
    expect(backslashAt("Documents/Old\\Work/Acme", platform)).toEqual({
      self: false,
      name: "Old\\Work",
    });
    // the deepest folder above it that has one, never one further up
    expect(backslashAt("/Volumes/x\\y/Clients\\2026/Acme/Notice", platform)).toEqual({
      self: false,
      name: "Clients\\2026",
    });
    expect(backslashAt("Documents/Acme", platform)).toBeNull();
    expect(backslashAt("", platform)).toBeNull();
    expect(backslashPlace("Documents/Old\\Work/Acme", platform)).toEqual({
      platform,
      parent: "Old\\Work",
    });
    expect(backslashPlace("Documents/Acme\\Sons", platform)).toEqual({ platform, parent: null });
    // no folder known: the folder's own name, as the line said before
    expect(backslashPlace(null, platform)).toEqual({ platform, parent: null });
  },
);

test("on Windows, no folder's name has one: the backslash is the separator", () => {
  expect(backslashAt("D:\\Matters\\Old\\Work\\Acme", "windows")).toBeNull();
  expect(backslashAt("Documents/Old\\Work/Acme", "windows")).toBeNull();
  expect(backslashPlace("Documents/Old\\Work/Acme", "windows")).toEqual({
    platform: "windows",
    parent: null,
  });
});
