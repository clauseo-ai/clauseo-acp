import { expect, test } from "vitest";
import { wordsOver } from "@clauseo/components/lib/words-core.ts";
import { ERROR_WORDS } from "@clauseo/components/lib/words-errors.ts";
import { currentOf, type Entry } from "./entry.ts";
import { REGISTRY } from "./registry.tsx";
import {
  duplicateIds,
  laterIds,
  laterLine,
  malformedIds,
  mergeApprovals,
  parseApprovals,
  pending,
  staleIds,
  statusOf,
  type Approval,
  type Approvals,
} from "./status.ts";

const approved = (en: string, hi: string | null): Approval => ({
  en,
  hi,
  ...(hi === null ? { hi_status: "later" as const } : {}),
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
    "U3-51",
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

// hi: later. A words list as a new screen's would hold it, one string with Hindi and two without, registered as the
// registry does, so the gate reads them through currentOf and each WordsRef's later.
const HEARING = {
  "Decided {date}": "निर्णय दिनांक {date}",
  "Hearing on {date}": null,
  "Hearing moved": null,
} as const;

const entriesOf = (hi: Record<keyof typeof HEARING, string | null>): Entry[] => {
  const say = wordsOver<keyof typeof HEARING>(() => hi);
  return (Object.keys(hi) as (keyof typeof HEARING)[]).map((key, i) => ({
    id: `T9-0${i + 1}`,
    words: {
      file: "lib/words-t9.ts",
      key,
      text: (lang) => say(lang, key),
      later: say.hi[key] === null,
    },
    screen: "test/t9",
    render: () => null,
  }));
};

const laterApprovals = parseApprovals({
  "T9-01": approved("Decided {date}", "निर्णय दिनांक {date}"),
  "T9-02": approved("Hearing on {date}", null),
  "T9-03": approved("Hearing moved", null),
});

test("the gate passes a string with null Hindi on an approval of its English with hi: null", () => {
  const rows = entriesOf(HEARING).map(currentOf);
  expect(rows.map((r) => r.hi)).toEqual(["निर्णय दिनांक {date}", null, null]);
  expect(pending(rows, laterApprovals)).toEqual([]);
  // the English is still approved: an English edit fails as before
  const moved = rows.map((r) => (r.id === "T9-02" ? { ...r, en: "Hearing on {date}." } : r));
  expect(pending(moved, laterApprovals)).toEqual([{ id: "T9-02", status: "changed" }]);
  // with no approval at all it is new, like any string
  const { "T9-02": _, ...rest } = laterApprovals;
  expect(pending(rows, rest)).toEqual([{ id: "T9-02", status: "new" }]);
});

test("once the words gain Hindi, an approval of hi: null fails as a mismatch until the Hindi is approved", () => {
  const rows = entriesOf({ ...HEARING, "Hearing on {date}": "सुनवाई {date} को" }).map(currentOf);
  expect(pending(rows, laterApprovals)).toEqual([{ id: "T9-02", status: "changed" }]);
  const now = { ...laterApprovals, "T9-02": approved("Hearing on {date}", "सुनवाई {date} को") };
  expect(pending(rows, parseApprovals(now))).toEqual([]);
});

test("a string with Hindi still needs its Hindi approved: hi: null doesn't cover it, nor does other Hindi", () => {
  const rows = entriesOf(HEARING).map(currentOf);
  const nullFor01 = { ...laterApprovals, "T9-01": approved("Decided {date}", null) };
  expect(pending(rows, parseApprovals(nullFor01))).toEqual([{ id: "T9-01", status: "changed" }]);
  const other = { ...laterApprovals, "T9-01": approved("Decided {date}", "निर्णय {date}") };
  expect(pending(rows, parseApprovals(other))).toEqual([{ id: "T9-01", status: "changed" }]);
  // and Hindi taken away again, back to null, no longer matches the approved Hindi
  const back = entriesOf({ ...HEARING, "Decided {date}": null }).map(currentOf);
  expect(pending(back, laterApprovals)).toEqual([{ id: "T9-01", status: "changed" }]);
});

test("the gate lists the hi: later strings by id in one line", () => {
  const rows = entriesOf(HEARING).map(currentOf);
  expect(laterIds(rows)).toEqual(["T9-02", "T9-03"]);
  expect(laterLine(rows)).toBe("hi: later: 2 strings: T9-02, T9-03");
  expect(laterLine(rows.slice(0, 2))).toBe("hi: later: 1 string: T9-02");
  expect(laterLine(rows.slice(0, 1))).toBe("hi: later: 0 strings");
});

test("an approval with hi: null needs hi_status later, and hi_status only goes with null Hindi", () => {
  const at = { status: "approved", at: "2026-11-03" };
  expect(() => parseApprovals({ "T9-02": { en: "a", hi: null, ...at } })).toThrow(/T9-02/);
  expect(() =>
    parseApprovals({ "T9-02": { en: "a", hi: "ब", hi_status: "later", ...at } }),
  ).toThrow(/T9-02/);
  expect(() =>
    parseApprovals({ "T9-02": { en: "a", hi: null, hi_status: "soon", ...at } }),
  ).toThrow(/T9-02/);
  expect(parseApprovals({ "T9-02": { en: "a", hi: null, hi_status: "later", ...at } })).toEqual({
    "T9-02": { en: "a", hi: null, hi_status: "later", ...at },
  });
});

test("an approved English with a provisional Hindi validates, and passes the gate as approved", () => {
  const row = {
    en: "Archived. Read only.",
    hi: "संग्रहीत। केवल पढ़ने के लिए।",
    hi_status: "provisional",
    hi_at: "2026-11-06",
    hi_by: "app track (provisional; Hindi-speaking lawyer to review)",
    status: "approved",
    at: "2026-11-04",
    by: "Rohan",
  } as const;
  expect(parseApprovals({ "AR-01": row })).toEqual({ "AR-01": row });
  expect(statusOf({ en: row.en, hi: row.hi }, row)).toBe("approved");
  // a provisional English takes a provisional Hindi the same way
  expect(() =>
    parseApprovals({
      "AR-01": { ...row, status: "provisional", at: "2026-11-05", by: "app track" },
    }),
  ).not.toThrow();
  // a provisional Hindi needs its Hindi, its date and who added it
  expect(() => parseApprovals({ "AR-01": { ...row, hi: null } })).toThrow(/AR-01/);
  for (const field of ["hi_at", "hi_by"] as const) {
    const { [field]: _, ...missing } = row;
    expect(() => parseApprovals({ "AR-01": missing }), field).toThrow(/AR-01/);
  }
  expect(() => parseApprovals({ "AR-01": { ...row, hi_at: "6 Nov" } })).toThrow(/AR-01/);
  expect(() => parseApprovals({ "AR-01": { ...row, hi_by: " " } })).toThrow(/AR-01/);
  // hi_at and hi_by go with hi_status provisional only
  const { hi_status: _, ...noStatus } = row;
  expect(() => parseApprovals({ "AR-01": noStatus })).toThrow(/AR-01/);
  expect(() => parseApprovals({ "AR-01": { ...row, hi: null, hi_status: "later" } })).toThrow(
    /AR-01/,
  );
  expect(() => parseApprovals({ "AR-01": { ...row, hi_status: "soon" } })).toThrow(/AR-01/);
});

// provisional: words that follow DESIGN.md, in use before Rohan's batch review.
const provisional = (en: string, hi: string | null): Approval => ({
  en,
  hi,
  status: "provisional",
  at: "2026-11-04",
  by: "app track",
});

test("a provisional entry whose text is current is provisional, and the gate lets it through", () => {
  expect(statusOf({ en: "Matters", hi: null }, provisional("Matters", null))).toBe("provisional");
  const rows = REGISTRY.map(currentOf);
  const [first] = rows;
  const some: Record<string, Approval> = { ...seedApprovals() };
  if (first !== undefined) some[first.id] = provisional(first.en, first.hi);
  expect(pending(rows, parseApprovals(some))).toEqual([]);
});

test("a provisional entry whose text has moved since is changed, and fails the gate", () => {
  expect(statusOf({ en: "Matter", hi: null }, provisional("Matters", null))).toBe("changed");
  const rows = REGISTRY.map(currentOf);
  const [first] = rows;
  const some: Record<string, Approval> = { ...seedApprovals() };
  if (first !== undefined) some[first.id] = provisional(`${first.en}!`, first.hi);
  expect(pending(rows, some)).toEqual([{ id: first?.id, status: "changed" }]);
});

test("an unknown status is refused", () => {
  for (const status of ["ok", "pending", "Provisional", undefined])
    expect(() => parseApprovals({ "U1-01": { ...provisional("a", "b"), status } })).toThrow(
      /U1-01/,
    );
});

test("a provisional entry needs en, hi (null allowed), at as a date and by", () => {
  const ok = provisional("a", null);
  expect(parseApprovals({ "U1-01": ok })).toEqual({ "U1-01": ok });
  expect(parseApprovals({ "U1-01": provisional("a", "ब") })).toEqual({
    "U1-01": provisional("a", "ब"),
  });
  expect(parseApprovals({ "U1-01": { ...ok, hi_status: "later" } })["U1-01"]?.hi).toBeNull();
  for (const field of ["en", "hi", "at", "by"] as const) {
    const { [field]: _, ...missing } = ok;
    expect(() => parseApprovals({ "U1-01": missing }), field).toThrow(/U1-01/);
  }
  for (const at of ["4 Nov", "2026-11-31", "2026-1-04", ""])
    expect(() => parseApprovals({ "U1-01": { ...ok, at } }), at).toThrow(/U1-01/);
  expect(() => parseApprovals({ "U1-01": { ...ok, by: " " } })).toThrow(/U1-01/);
  expect(() => parseApprovals({ "U1-01": { ...ok, hi: "ब", hi_status: "later" } })).toThrow(
    /U1-01/,
  );
});

test("the loader merges the folder's files into one map", () => {
  const merged = mergeApprovals({
    "../../copy-approvals/U1.json": { "U1-01": approved("Matters", "मामले") },
    "../../copy-approvals/U3.json": {
      "U3-01": approved("Stopped", null),
      "U3-02": provisional("Sign in again.", null),
    },
  });
  expect(merged).toEqual({
    "U1-01": approved("Matters", "मामले"),
    "U3-01": approved("Stopped", null),
    "U3-02": provisional("Sign in again.", null),
  });
});

test("the loader refuses an id in two files, naming both", () => {
  expect(() =>
    mergeApprovals({
      "copy-approvals/U1.json": { "U1-01": approved("Matters", "मामले") },
      "copy-approvals/other.json": { "U1-01": approved("Matters", "मामले") },
    }),
  ).toThrow(
    "copy approvals: U1-01 is in both copy-approvals/U1.json and copy-approvals/other.json",
  );
});

test("the loader parses each file strictly, naming the file", () => {
  expect(() =>
    mergeApprovals({
      "copy-approvals/U1.json": { "U1-01": { en: "Matters", status: "approved" } },
    }),
  ).toThrow("copy-approvals/U1.json: U1-01 needs");
  expect(() => mergeApprovals({ "copy-approvals/U1.json": [] })).toThrow(
    "copy-approvals/U1.json must be an object",
  );
});

test("an empty folder is no approvals, so every string is new", () => {
  expect(mergeApprovals({})).toEqual({});
  const rows = REGISTRY.map(currentOf);
  expect(pending(rows, mergeApprovals({})).every((r) => r.status === "new")).toBe(true);
});
