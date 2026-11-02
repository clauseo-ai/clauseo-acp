import { expect, test } from "vitest";
import { ERROR_WORDS } from "@clauseo/components/lib/words-errors.ts";
import { currentOf } from "./entry.ts";
import { REGISTRY } from "./registry.tsx";
import {
  duplicateIds,
  malformedIds,
  parseApprovals,
  pending,
  staleIds,
  statusOf,
  type Approval,
  type Approvals,
} from "./status.ts";

const approved = (en: string, hi: string): Approval => ({
  en,
  hi,
  status: "approved",
  at: "2026-11-02",
});

test("a string is approved while the approval's text is the current English and Hindi", () => {
  expect(statusOf({ en: "Matters", hi: "मामले" }, approved("Matters", "मामले"))).toBe("approved");
});

test("an approval whose English or Hindi has moved since is changed", () => {
  expect(statusOf({ en: "Matters", hi: "मामले" }, approved("Matter", "मामले"))).toBe("changed");
  expect(statusOf({ en: "Matters", hi: "मामला" }, approved("Matters", "मामले"))).toBe("changed");
});

test("a requested change stands while the text is the one it was asked on", () => {
  const change: Approval = { ...approved("Matters", "मामले"), status: "change", note: "Shorter" };
  expect(statusOf({ en: "Matters", hi: "मामले" }, change)).toBe("change");
});

test("a requested change whose English or Hindi has moved since is changed", () => {
  const change: Approval = { ...approved("Matters", "मामले"), status: "change", note: "Shorter" };
  expect(statusOf({ en: "Files", hi: "मामले" }, change)).toBe("changed");
  expect(statusOf({ en: "Matters", hi: "फ़ाइलें" }, change)).toBe("changed");
});

test("a string with no approval is new", () => {
  expect(statusOf({ en: "Matters", hi: "मामले" }, undefined)).toBe("new");
});

/** Approvals of every seeded string as the words files have it now. */
const seedApprovals = (): Record<string, Approval> =>
  Object.fromEntries(REGISTRY.map(currentOf).map((c) => [c.id, approved(c.en, c.hi)]));

test("the gate passes with every seeded string approved as it reads now", () => {
  expect(REGISTRY.length).toBeGreaterThan(0);
  expect(pending(REGISTRY.map(currentOf), seedApprovals())).toEqual([]);
});

test("the gate fails once a seeded string's text changes after its approval", () => {
  const rows = REGISTRY.map(currentOf);
  const moved = rows.map((r, i) => (i === 0 ? { ...r, en: `${r.en}!` } : r));
  expect(pending(moved, seedApprovals())).toEqual([{ id: rows[0]?.id, status: "changed" }]);
});

test("the gate lists new strings and requested changes", () => {
  const rows = REGISTRY.map(currentOf);
  const [first, second] = rows;
  const approvals: Approvals = { ...seedApprovals() };
  const some = { ...approvals };
  if (first !== undefined) delete some[first.id];
  if (second !== undefined)
    some[second.id] = { ...approved(second.en, second.hi), status: "change", note: "x" };
  expect(pending(rows, some)).toEqual([
    { id: first?.id, status: "new" },
    { id: second?.id, status: "change" },
  ]);
});

test("approvals of ids the registry no longer has are listed as stale", () => {
  const approvals = { ...seedApprovals(), "U1-99": approved("Gone", "गया") };
  expect(staleIds(REGISTRY, approvals)).toEqual(["U1-99"]);
  expect(staleIds(REGISTRY, seedApprovals())).toEqual([]);
});

test("registry ids are unique and typeable", () => {
  expect(duplicateIds(REGISTRY)).toEqual([]);
  expect(malformedIds(REGISTRY)).toEqual([]);
  expect(duplicateIds([{ id: "U1-01" }, { id: "U1-01" }])).toEqual(["U1-01"]);
  expect(malformedIds([{ id: "U1-1" }, { id: "u1-01" }, { id: "U1-01" }])).toEqual([
    "U1-1",
    "u1-01",
  ]);
  // an error's sentence goes by its code
  expect(
    malformedIds([
      { id: "U3-err-store.disk_full" },
      { id: "U3-err-unknown" },
      { id: "U3-err-" },
      { id: "U3-err-Store.full" },
      { id: "U3-err-a..b" },
    ]),
  ).toEqual(["U3-err-", "U3-err-Store.full", "U3-err-a..b"]);
});

test("U3's ids are written as they are: one per error code but U3-07's, and the limit row's own Reset not given.", () => {
  const ids = REGISTRY.map((e) => e.id).filter((id) => id.startsWith("U3-"));
  const codes = (Object.entries(ERROR_WORDS) as [string, string][])
    .filter(
      ([, key]) => key !== "The answer stopped. Your question and the answer so far are kept.",
    )
    .map(([code]) => `U3-err-${code}`);
  expect(ids.filter((id) => id.startsWith("U3-err-"))).toEqual(codes);
  expect(ids.filter((id) => !id.startsWith("U3-err-"))).toEqual([
    ...Array.from({ length: 15 }, (_, i) => `U3-${String(i + 1).padStart(2, "0")}`),
    "U3-17",
    "U3-18",
    "U3-48",
    "U3-49",
    "U3-50",
  ]);
  expect(REGISTRY.find((e) => e.id === "U3-48")?.words.key).toBe("Reset not given.");
});

test("approvals of the wrong shape are refused by id", () => {
  expect(() => parseApprovals({ "U1-01": { en: "Matters", status: "approved", at: "x" } })).toThrow(
    /U1-01/,
  );
  expect(() => parseApprovals({ "U1-01": { ...approved("a", "b"), status: "ok" } })).toThrow(
    /U1-01/,
  );
  expect(() => parseApprovals([])).toThrow();
  expect(parseApprovals({ "U1-01": approved("a", "b") })).toEqual({ "U1-01": approved("a", "b") });
});
