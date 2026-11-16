import { expect, test } from "vitest";
import { findMatters } from "./rail-find.ts";

const MATTERS = [
  { id: "acme", name: "Acme share purchase", client: "Acme Holdings" },
  { id: "rao", name: "Rao Family Trust", client: "Asha Rao" },
  { id: "bharat", name: "Bharat Mills lease", client: "Kiran Rao" },
] as const;

test("a matter is found by name or client, names first, ignoring case, spaces and dots", () => {
  expect(findMatters(MATTERS, "rao").map((f) => [f.matter.id, f.by])).toEqual([
    ["rao", "name"],
    ["bharat", "client"],
  ]);
  expect(findMatters(MATTERS, "acme hold").map((f) => [f.matter.id, f.by])).toEqual([
    ["acme", "client"],
  ]);
  expect(findMatters(MATTERS, "a.c.m.e").map((f) => f.matter.id)).toEqual(["acme"]);
});

test("a search that matches nothing, or holds only spaces, finds nothing", () => {
  expect(findMatters(MATTERS, "zebra")).toEqual([]);
  expect(findMatters(MATTERS, "  ")).toEqual([]);
  expect(findMatters(MATTERS, "")).toEqual([]);
});
