// node --test scripts/bundle-hash.test.mjs: the deploy skip's hash changes with any shipped byte
// and with nothing else, and the last deploy's hash is found only where it is safe to trust.

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { ARTIFACT, BUNDLE, bundleFiles, bundleHash, lastDeployed } from "./bundle-hash.mjs";

/** A worker folder holding every BUNDLE entry, with `files` written over it. */
function folder(t, files = {}) {
  const root = mkdtempSync(join(tmpdir(), "bundle-hash-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const all = {
    "build/worker/shim.mjs": "export default {}",
    "build/index.wasm": "\0asm",
    "migrations/0001_init.sql": "CREATE TABLE report (install TEXT);",
    "migrations/0002_rollup.sql": "CREATE TABLE tally (tally TEXT);",
    ...Object.fromEntries(BUNDLE.filter((e) => e.includes(".")).map((e) => [e, `${e}\n`])),
    ...files,
  };
  for (const [rel, text] of Object.entries(all)) {
    mkdirSync(join(root, rel, ".."), { recursive: true });
    writeFileSync(join(root, rel), text);
  }
  return root;
}

test("the same files give the same hash, and the list is sorted and complete", (t) => {
  const a = folder(t);
  const b = folder(t);
  assert.equal(bundleHash(a), bundleHash(b));
  const files = bundleFiles(a);
  assert.deepEqual(files, [...files].sort());
  assert.ok(files.includes("build/worker/shim.mjs"));
  assert.ok(files.includes("migrations/0002_rollup.sql"));
  assert.ok(files.includes("wrangler.toml"));
  assert.ok(!files.some((f) => f.endsWith(".test.mjs")));
});

test("every script the deploy job runs, and all they import, is hashed", () => {
  const here = dirname(fileURLToPath(import.meta.url));
  const seen = new Set();
  const visit = (name) => {
    if (seen.has(name)) return;
    seen.add(name);
    const text = readFileSync(join(here, name), "utf8");
    for (const [, dep] of text.matchAll(/from "\.\/([\w-]+\.mjs)"/g)) visit(dep);
  };
  // The scripts counts-deploy.yml runs; check-access.mjs runs on Rohan's Mac, not in the job.
  for (const name of ["deploy.mjs", "check-deploy.mjs", "bundle-hash.mjs"]) visit(name);
  assert.ok(seen.has("access.mjs") && seen.has("r2.mjs") && seen.has("cloudflare.mjs"));
  for (const name of seen) assert.ok(BUNDLE.includes(`scripts/${name}`), name);
  assert.ok(!BUNDLE.includes("scripts/check-access.mjs"));
});

test("the order the files are given in does not matter", (t) => {
  const root = folder(t);
  const files = bundleFiles(root);
  assert.equal(bundleHash(root, [...files].reverse()), bundleHash(root, files));
  assert.deepEqual(bundleFiles(root, [...BUNDLE].reverse()), files);
});

test("a changed, added or renamed migration changes the hash", (t) => {
  const base = bundleHash(folder(t));
  assert.notEqual(bundleHash(folder(t, { "migrations/0002_rollup.sql": "CREATE TABLE tally (n INT);" })), base);
  assert.notEqual(bundleHash(folder(t, { "migrations/0005_more.sql": "" })), base);
  const renamed = folder(t);
  rmSync(join(renamed, "migrations/0002_rollup.sql"));
  writeFileSync(join(renamed, "migrations/0003_rollup.sql"), "CREATE TABLE tally (tally TEXT);");
  assert.notEqual(bundleHash(renamed), base);
});

test("a changed bundle, config or deploy script changes the hash", (t) => {
  const base = bundleHash(folder(t));
  for (const rel of ["build/index.wasm", "wrangler.toml", "scripts/deploy.mjs", "pnpm-lock.yaml"]) {
    assert.notEqual(bundleHash(folder(t, { [rel]: "changed" })), base, rel);
  }
});

test("bytes moved between two files' names and contents change the hash", (t) => {
  const a = folder(t, { "migrations/0001_init.sql": "ab", "migrations/0002_rollup.sql": "c" });
  const b = folder(t, { "migrations/0001_init.sql": "a", "migrations/0002_rollup.sql": "bc" });
  assert.notEqual(bundleHash(a), bundleHash(b));
});

test("a hidden file is left out, as the artifact leaves it out", (t) => {
  const plain = folder(t);
  const hidden = folder(t, { "build/.gitignore": "*" });
  assert.ok(!bundleFiles(hidden).includes("build/.gitignore"));
  assert.equal(bundleHash(hidden), bundleHash(plain));
});

test("a missing entry or a symlink fails", (t) => {
  const missing = folder(t);
  rmSync(join(missing, "scripts/r2.mjs"));
  assert.throws(() => bundleFiles(missing), /scripts\/r2\.mjs: missing/);
  const linked = folder(t);
  symlinkSync("/etc/hostname", join(linked, "build/link"));
  assert.throws(() => bundleFiles(linked), /neither/);
});

const HASH = "a".repeat(64);
const REPO = "o/r";

/** An API over `runs` ([id, deploy job conclusion or null, artifact?]), newest first. */
function api(runs, { expired = false } = {}) {
  return (path) => {
    if (path.includes("/workflows/")) {
      const page = Number(path.match(/page=(\d+)$/)[1]);
      return { workflow_runs: page === 1 ? runs.map(([id]) => ({ id })) : [] };
    }
    const id = Number(path.match(/runs\/(\d+)\//)[1]);
    const [, conclusion, artifact] = runs.find(([run]) => run === id);
    if (path.includes("/jobs")) {
      const jobs = [{ name: "bundle", conclusion: "success" }];
      if (conclusion) jobs.push({ name: "deploy", conclusion });
      return { jobs };
    }
    return { artifacts: artifact ? [{ name: ARTIFACT, expired }] : [] };
  };
}
const where = { repo: REPO, runId: 99 };

test("the newest run whose deploy ran gives the hash; skipped deploys are passed over", () => {
  const runs = [[99, null], [5, "skipped"], [4, null], [3, "success", true], [2, "success", true]];
  const seen = [];
  const got = lastDeployed(api(runs), (id) => (seen.push(id), `${HASH}\n`), where);
  assert.equal(got, HASH);
  assert.deepEqual(seen, [3]);
});

test("a failed or cancelled deploy, or one with no hash, means deploy again", () => {
  const dl = () => HASH;
  for (const conclusion of ["failure", "cancelled"]) {
    assert.equal(lastDeployed(api([[4, conclusion], [3, "success", true]]), dl, where), undefined, conclusion);
  }
  assert.equal(lastDeployed(api([[4, "success"], [3, "success", true]]), dl, where), undefined);
  assert.equal(lastDeployed(api([[3, "success", true]], { expired: true }), dl, where), undefined);
  assert.equal(lastDeployed(api([[3, "success", true]]), () => "not a hash", where), undefined);
  assert.equal(lastDeployed(api([[5, "skipped"]]), dl, where), undefined);
  assert.equal(lastDeployed(() => ({}), dl, where), undefined);
});
