// Tests for artifact-sums.mjs, which carries each artifact's SHA-256s from the job that uploads it
// to the job that downloads it (release.yml). Plain Node, no install:
//
//   node --test .github/scripts/artifact-sums.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { artifactSums, sumsFromOutputs, sumsProblems } from "./artifact-sums.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const sha = (text) => createHash("sha256").update(text).digest("hex");
const run = (...args) => spawnSync(process.execPath, [join(here, "artifact-sums.mjs"), ...args], { encoding: "utf8" });

function tree() {
  const dir = mkdtempSync(join(tmpdir(), "artifact-sums-"));
  mkdirSync(join(dir, "crates", "binaries"), { recursive: true });
  writeFileSync(join(dir, "Clauseo.exe"), "exe");
  writeFileSync(join(dir, "crates", "binaries", "tool.exe"), "tool");
  writeFileSync(join(dir, "crates", "overlay.json"), "{}");
  return dir;
}

test("artifactSums lists every file given, folders walked, by its path from the root", () => {
  const dir = tree();
  try {
    symlinkSync(join(dir, "Clauseo.exe"), join(dir, "crates", "link.exe"));
    assert.deepEqual(artifactSums(dir, [join(dir, "Clauseo.exe"), join(dir, "crates")]), {
      "Clauseo.exe": sha("exe"),
      "crates/binaries/tool.exe": sha("tool"),
      "crates/overlay.json": sha("{}"),
    });
    assert.throws(() => artifactSums(join(dir, "crates"), [join(dir, "Clauseo.exe")]), /is not inside/);
    assert.throws(() => artifactSums(dir, [join(dir, "missing")]), /no such file/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("sumsProblems names each missing, extra and changed file", () => {
  const a = sha("a");
  assert.deepEqual(sumsProblems({ x: a, y: a }, { x: a, y: a }), []);
  assert.deepEqual(
    sumsProblems({ x: a, y: a }, { x: sha("b"), z: a }).map((line) => line.replace(/:.*/, "")),
    ["x", "y", "z"],
  );
});

test("sumsFromOutputs takes one leg's sums from a job's outputs, and refuses none", () => {
  const sums = { "a.exe": sha("a") };
  const outputs = JSON.stringify({ "release-x": JSON.stringify(sums), "release-y": "" });
  assert.deepEqual(sumsFromOutputs(outputs, "release-x"), sums);
  assert.throws(() => sumsFromOutputs(outputs, "release-y"), /set no output release-y/);
  assert.throws(() => sumsFromOutputs(outputs, "release-z"), /set no output/);
  assert.throws(() => sumsFromOutputs("", "release-x"), /set no output/);
  assert.throws(() => sumsFromOutputs(JSON.stringify({ k: "{}" }), "k"), /lists no files/);
  assert.throws(() => sumsFromOutputs(JSON.stringify({ k: '{"a":"nope"}' }), "k"), /the digest/);
});

test("sums, then check, pass for the same files and fail for any change", () => {
  const dir = tree();
  try {
    const paths = [join(dir, "Clauseo.exe"), join(dir, "crates")];
    const sums = run("sums", dir, ...paths);
    assert.equal(sums.status, 0, sums.stderr);
    assert.equal(sums.stdout.trim().split("\n").length, 1, "one line, for $GITHUB_OUTPUT");
    const outputs = JSON.stringify({ "windows-exe-t": sums.stdout.trim() });
    const ok = run("check", outputs, "windows-exe-t", dir, ...paths);
    assert.equal(ok.status, 0, ok.stderr);
    assert.match(ok.stdout, /windows-exe-t: 3 files, each as uploaded/);
    writeFileSync(join(dir, "crates", "binaries", "tool.exe"), "another tool");
    const changed = run("check", outputs, "windows-exe-t", dir, ...paths);
    assert.equal(changed.status, 1);
    assert.match(changed.stderr, /crates\/binaries\/tool\.exe: SHA-256 /);
    writeFileSync(join(dir, "crates", "binaries", "tool.exe"), "tool");
    writeFileSync(join(dir, "crates", "extra.dll"), "extra");
    assert.match(run("check", outputs, "windows-exe-t", dir, ...paths).stderr, /extra\.dll: downloaded, but/);
    assert.equal(run("check", outputs, "release-t", dir, ...paths).status, 1);
    assert.equal(run("check", outputs, "windows-exe-t", dir, join(dir, "missing")).status, 2);
    assert.equal(run("sums", dir).status, 2);
    assert.equal(run("nothing").status, 2);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
