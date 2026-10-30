import { expect, test } from "vitest";
import { findMatters } from "./rail-find.ts";

const MATTERS = [
  { id: "acme", name: "Acme Ltd v. State", client: "Acme Ltd", caseNo: "CC No. 1001/2024" },
  { id: "rao", name: "Rao Family Trust", client: "Asha Rao", caseNo: "OS 77/2023" },
  {
    id: "bharat",
    name: "Bharat Mills v. Union of India",
    client: "Kiran Rao",
    caseNo: "WP 9921/2025",
  },
] as const;

test("a matter is found by name, client or case number, names first, ignoring case, spaces and dots", () => {
  expect(findMatters(MATTERS, "rao").map((f) => [f.matter.id, f.by])).toEqual([
    ["rao", "name"],
    ["bharat", "client"],
  ]);
  expect(findMatters(MATTERS, "cc no 1001").map((f) => f.matter.id)).toEqual(["acme"]);
  expect(findMatters(MATTERS, "9921").map((f) => f.by)).toEqual(["caseNo"]);
});

test("a search that matches nothing, or holds only spaces, finds nothing", () => {
  expect(findMatters(MATTERS, "zebra")).toEqual([]);
  expect(findMatters(MATTERS, "  ")).toEqual([]);
  expect(findMatters(MATTERS, "")).toEqual([]);
});
