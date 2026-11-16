// folder-wildcard.ts: matter.folder_wildcard's line names the deepest folder in the matter folder's path whose name has
// a * or ?, the matter folder's own name or one above it; none known or none in sight names none.
import { expect, test } from "vitest";
import { WILDCARDS, wildcardAt, wildcardName } from "./folder-wildcard.ts";

test("the wildcard characters are the host's: * and ?", () => {
  expect(WILDCARDS).toEqual(["*", "?"]);
});

test("the matter folder's own name, when it has one", () => {
  expect(wildcardAt("~/Documents/stagec-1097/Test*")).toBe("Test*");
  expect(wildcardAt("Documents/What?")).toBe("What?");
  expect(wildcardAt("Documents/Acme*/Notice?/")).toBe("Notice?");
});

test("a folder above it, the deepest one that has one", () => {
  expect(wildcardAt("Documents/Clients*/Acme")).toBe("Clients*");
  expect(wildcardAt("/Volumes/x?y/Clients*2026/Acme/Notice")).toBe("Clients*2026");
});

test("none in sight, or no folder known: no name", () => {
  expect(wildcardAt("Documents/Acme Traders")).toBeNull();
  expect(wildcardAt("")).toBeNull();
  expect(wildcardName("~/Documents/Acme")).toBeNull();
  expect(wildcardName(null)).toBeNull();
  expect(wildcardName("Documents/Test*")).toBe("Test*");
});
