// The auto-registered entries (registry-auto.ts): the glob finds review/auto/'s files, keeps their order whatever
// order it is handed them in, refuses a file whose default export is not a list, and the registry ends with them, so
// the copy gate's duplicate check (copy-gate.check.ts) sees an id in both an auto file and the hand-kept list.
import { expect, test } from "vitest";
import type { Entry } from "./entry.ts";
import { AUTO_ENTRIES, AUTO_FILES, collectEntries } from "./registry-auto.ts";
import { REGISTRY } from "./registry.tsx";
import { duplicateIds } from "./status.ts";

const entry = (id: string): Entry => ({
  id,
  words: { file: "test", key: id, text: () => id, later: true },
  screen: "test/none",
  render: () => null,
});

test("the glob finds review/auto/'s files, its tests left out", () => {
  expect(AUTO_FILES).toContain("./auto/README.tsx");
  expect(AUTO_FILES.filter((p) => p.endsWith(".test.tsx"))).toEqual([]);
});

test("entries come in path order, whatever order the files are handed in", () => {
  const b = { default: [entry("B-01"), entry("B-02")] };
  const a = { default: [entry("A-01")] };
  const ids = (files: Record<string, unknown>) => collectEntries(files).map((e) => e.id);
  expect(ids({ "./auto/b.tsx": b, "./auto/a.tsx": a })).toEqual(["A-01", "B-01", "B-02"]);
  expect(ids({ "./auto/a.tsx": a, "./auto/b.tsx": b })).toEqual(["A-01", "B-01", "B-02"]);
});

test("a file whose default export is not a list fails, naming the file", () => {
  expect(() => collectEntries({ "./auto/one.tsx": { default: entry("A-01") } })).toThrow(
    "./auto/one.tsx",
  );
  expect(() => collectEntries({ "./auto/named.tsx": { ENTRIES: [] } })).toThrow("./auto/named.tsx");
});

test("the registry ends with the auto entries, so an id in both sources fails the duplicate check", () => {
  expect(REGISTRY.slice(REGISTRY.length - AUTO_ENTRIES.length)).toEqual(AUTO_ENTRIES);
  const taken = REGISTRY[0]!.id;
  const auto = collectEntries({ "./auto/dup.tsx": { default: [entry(taken)] } });
  const hand = REGISTRY.slice(0, REGISTRY.length - AUTO_ENTRIES.length);
  expect(duplicateIds([...hand, ...auto])).toEqual([taken]);
});
