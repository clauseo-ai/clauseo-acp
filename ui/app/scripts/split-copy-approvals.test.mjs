import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, test } from "vitest";
import { fileOf, splitApprovals, splitFile } from "./split-copy-approvals.mjs";

const at = "2026-11-02";
const FIXTURE = {
  "U1-01": { en: "Matters", hi: "मामले", status: "approved", at, by: "design" },
  "UTC-01": { en: "Thinking", hi: null, hi_status: "later", status: "approved", at },
  "U1-02": { en: "No matters yet", hi: "अभी कोई मामला नहीं", status: "approved", at, note: "Rohan" },
  "U16a-01": { en: "Needs you", hi: null, hi_status: "later", status: "change", at, note: "x" },
  "U3-err-signin.expired": {
    en: "Sign in again.",
    hi: null,
    hi_status: "later",
    status: "approved",
    at: "2026-11-04",
    by: "app track (provisional; Rohan batch review)",
  },
  "FR-01": {
    en: "Folder",
    hi: null,
    hi_status: "later",
    status: "approved",
    at: "2026-11-05",
    by: "stage C (provisional; Rohan batch review)",
  },
  LONE: { en: "Lone", hi: "अकेला", status: "approved", at },
};

const dirs = [];
afterEach(() => {
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});

test("an id goes in the file named by its prefix before the first dash, or other.json", () => {
  expect(fileOf("U1-01")).toBe("U1.json");
  expect(fileOf("U16a-01")).toBe("U16a.json");
  expect(fileOf("U3-err-store.disk_full")).toBe("U3.json");
  expect(fileOf("LONE")).toBe("other.json");
  expect(fileOf("-01")).toBe("other.json");
});

test("the split keeps each entry's shape and order within its file, written as the old file was", () => {
  const files = splitApprovals(JSON.stringify(FIXTURE, null, 2));
  expect([...files.keys()]).toEqual([
    "U1.json",
    "UTC.json",
    "U16a.json",
    "U3.json",
    "FR.json",
    "other.json",
  ]);
  const u1 = files.get("U1.json");
  expect(u1).toBe(
    `${JSON.stringify({ "U1-01": FIXTURE["U1-01"], "U1-02": FIXTURE["U1-02"] }, null, 2)}\n`,
  );
  expect(Object.keys(JSON.parse(files.get("UTC.json"))["UTC-01"])).toEqual([
    "en",
    "hi",
    "hi_status",
    "status",
    "at",
  ]);
  expect(JSON.parse(files.get("other.json"))).toEqual({ LONE: FIXTURE.LONE });
});

test("an approved entry whose by names provisional becomes provisional, its by kept", () => {
  const u3 = JSON.parse(splitApprovals(JSON.stringify(FIXTURE)).get("U3.json"));
  expect(u3).toEqual({
    "U3-err-signin.expired": {
      en: "Sign in again.",
      hi: null,
      hi_status: "later",
      status: "provisional",
      at: "2026-11-04",
      by: "app track (provisional; Rohan batch review)",
    },
  });
  // another thread's entry keeps its own by
  expect(JSON.parse(splitApprovals(JSON.stringify(FIXTURE)).get("FR.json"))["FR-01"]).toEqual({
    ...FIXTURE["FR-01"],
    status: "provisional",
  });
  // the keys stay where they were
  expect(Object.keys(u3["U3-err-signin.expired"])).toEqual(
    Object.keys(FIXTURE["U3-err-signin.expired"]),
  );
});

test("splitFile writes the folder and removes the old file; it refuses a folder already split unless forced", () => {
  const dir = mkdtempSync(join(tmpdir(), "split-approvals-"));
  dirs.push(dir);
  const from = join(dir, "copy-approvals.json");
  const to = join(dir, "copy-approvals");
  writeFileSync(from, `${JSON.stringify(FIXTURE, null, 2)}\n`);
  expect(splitFile({ from, to })).toHaveLength(6);
  expect(existsSync(from)).toBe(false);
  expect(readdirSync(to).sort()).toEqual([
    "FR.json",
    "U1.json",
    "U16a.json",
    "U3.json",
    "UTC.json",
    "other.json",
  ]);
  // no old file: nothing to do
  expect(splitFile({ from, to })).toBeNull();
  // a rebase brings the old file back with fewer briefs: refused, then replaced with --force
  writeFileSync(from, JSON.stringify({ "U1-01": FIXTURE["U1-01"] }));
  expect(() => splitFile({ from, to })).toThrow(/--force/);
  expect(splitFile({ from, to, force: true, keep: true })).toEqual(["U1.json"]);
  expect(readdirSync(to)).toEqual(["U1.json"]);
  expect(existsSync(from)).toBe(true);
  expect(JSON.parse(readFileSync(join(to, "U1.json"), "utf8"))).toEqual({
    "U1-01": FIXTURE["U1-01"],
  });
});
