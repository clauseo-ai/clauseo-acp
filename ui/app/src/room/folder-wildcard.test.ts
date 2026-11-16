// folder-wildcard.ts: matter.folder_wildcard's line names the deepest folder in the matter folder's path whose name has
// a * or ?, the matter folder's own name or one above it; none known or none in sight names none.
import { expect, test } from "vitest";
import {
  WILDCARDS,
  coveringWildcard,
  denyCovers,
  wildcardAt,
  wildcardName,
} from "./folder-wildcard.ts";

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

// Claude Code 2.1.293's own converter (sandbox-runtime 0.0.77/0.0.78's globToRegex): * and ? only ever cover more than
// the folder they came from; [ is escaped as [[] by the host, so [x] covers itself
test("a deny covers its own folder and what is beneath it, and more", () => {
  for (const folder of [
    "/x/What?",
    "/x/A*B",
    "/x/stagec-1097/Test*",
    "/x/stagec-proof-claude-[x]",
    "/x/a\\b",
  ]) {
    expect(denyCovers(folder, folder)).toBe(true);
    expect(denyCovers(folder, `${folder}/inner`)).toBe(true);
  }
  expect(denyCovers("/x/?", "/x/A")).toBe(true);
  expect(denyCovers("/x/?", "/x/AB")).toBe(false);
  expect(denyCovers("/x/A*B", "/x/A/B")).toBe(false);
  expect(denyCovers("/x/stagec-proof-claude-[x]", "/x/stagec-proof-claude-x")).toBe(false);
});

test("agent.sandbox_wildcard's folder: the other matter whose pattern would cover this one's", () => {
  expect(coveringWildcard("Documents/Acme", ["Documents/Ac?e"])).toBe("Ac?e");
  expect(coveringWildcard("Documents/Acme", ["Documents/What?", "Documents/A*"])).toBe("A*");
  expect(coveringWildcard("Documents/Clients/Acme", ["Documents/Cl*"])).toBe("Cl*");
  // one that wouldn't cover it, one with no wildcard, or none: no name
  expect(coveringWildcard("Documents/Acme", ["Documents/What?", "Documents/Acme"])).toBeNull();
  expect(coveringWildcard("Documents/Acme", ["/Volumes/A*"])).toBeNull();
  expect(coveringWildcard(null, ["Documents/A*"])).toBeNull();
  expect(coveringWildcard("Documents/Acme", [undefined])).toBeNull();
});
