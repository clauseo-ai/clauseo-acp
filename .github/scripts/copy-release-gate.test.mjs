// Tests for copy-release-gate.mjs, the gate a v* release runs on ui/app/copy-approvals. Plain
// Node, no install, no network:
//
//   node --test .github/scripts/copy-release-gate.test.mjs
//
// ci.yml's fast lane runs it beside latest-json.test.mjs.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { offenders, report } from "./copy-release-gate.mjs";

const SCRIPT = join(dirname(fileURLToPath(import.meta.url)), "copy-release-gate.mjs");

const approved = (extra = {}) => ({
  en: "Open",
  hi: "खोलें",
  status: "approved",
  at: "2026-11-01",
  ...extra,
});
const hiProvisional = { hi_status: "provisional", hi_at: "2026-11-12", hi_by: "app track" };

/** Runs the script on a folder holding files, returning its exit code and output. */
function run(files) {
  const dir = mkdtempSync(join(tmpdir(), "copy-release-gate-"));
  try {
    for (const [name, entries] of Object.entries(files))
      writeFileSync(join(dir, name), JSON.stringify(entries));
    const out = spawnSync(process.execPath, [SCRIPT, dir], { encoding: "utf8" });
    return { code: out.status, text: out.stdout + out.stderr };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("all approved passes", () => {
  const out = run({
    "U1.json": { "U1-01": approved(), "U1-02": approved({ hi: null, hi_status: "later" }) },
  });
  assert.equal(out.code, 0, out.text);
  assert.match(out.text, /all 2 entries in 1 files are approved/);
  assert.doesNotMatch(out.text, /::error::/);
});

test("a provisional entry fails", () => {
  const out = run({
    "U1.json": {
      "U1-01": approved(),
      "U1-02": approved({ status: "provisional", by: "app track" }),
    },
  });
  assert.equal(out.code, 1);
  assert.match(out.text, /^::error::copy release gate: 1 entry is not approved/m);
  assert.match(out.text, /^status "provisional", not "approved": 1\n {2}U1-02 \(U1\.json\)$/m);
  assert.doesNotMatch(out.text, /U1-01/);
});

test("an entry marked change fails", () => {
  const out = run({ "MN.json": { "MN-03": approved({ status: "change" }) } });
  assert.equal(out.code, 1);
  assert.match(out.text, /^status "change", not "approved": 1\n {2}MN-03 \(MN\.json\)$/m);
});

test("provisional Hindi fails, even on approved English", () => {
  const out = run({ "U3.json": { "U3-err-1": approved(hiProvisional) } });
  assert.equal(out.code, 1);
  assert.match(out.text, /^Hindi "hi_status": "provisional": 1\n {2}U3-err-1 \(U3\.json\)$/m);
  assert.doesNotMatch(out.text, /not "approved"/);
});

test("Hindi marked later passes", () => {
  const out = run({ "U3.json": { "U3-01": approved({ hi: null, hi_status: "later" }) } });
  assert.equal(out.code, 0, out.text);
});

test("every offending id is listed, grouped by reason, with one ::error:: line", () => {
  const out = run({
    "UC-x.json": {
      "UC-250": approved({ status: "provisional", ...hiProvisional }),
      "UC-251": approved(),
    },
    "UC.json": { "UC-001": approved({ status: "change" }), "UC-002": approved(hiProvisional) },
  });
  assert.equal(out.code, 1);
  assert.equal(out.text.match(/::error::/g).length, 1);
  assert.match(out.text, /3 entries are not approved/);
  assert.deepEqual(
    offenders({ "a.json": { A: approved({ status: "provisional", ...hiProvisional }) } }),
    {
      'status "provisional", not "approved"': ["A (a.json)"],
      'Hindi "hi_status": "provisional"': ["A (a.json)"],
    },
  );
  for (const id of ["UC-250 (UC-x.json)", "UC-001 (UC.json)", "UC-002 (UC.json)"])
    assert.ok(out.text.includes(`  ${id}\n`), id);
  assert.doesNotMatch(out.text, /UC-251/);
});

test("a file that is not an object of entries fails", () => {
  assert.equal(report({ "bad.json": [] }).code, 1);
});
